//! MCP bridge: spawn MCP servers as subprocesses, register their tools in ToolManager.
//!
//! Each MCP server runs as a child process (stdin/stdout JSON-RPC 2.0).
//! Tools are discovered via `tools/list` and proxied via `tools/call`.
//! Sessions are stored in a global map, keyed by tool name.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{ToolHandler, ToolKey, ToolCallCtx, ToolResult, SafetyVerdict};

/// Configuration for one MCP server.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct McpServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Global MCP session registry — keyed by tool name.
static MCP_SESSIONS: Mutex<Option<HashMap<String, Arc<McpSession>>>> = Mutex::new(None);

/// A connected MCP server process.
struct McpSession {
    child: Mutex<Child>,
    stdin: Mutex<Box<dyn Write + Send>>,
    stdout: Mutex<BufReader<Box<dyn std::io::Read + Send>>>,
    id_counter: Mutex<u64>,
}

/// Send a JSON-RPC request and read the response.
fn rpc_call(
    session: &McpSession,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let id = {
        let mut c = session.id_counter.lock().expect("lock");
        *c += 1;
        *c
    };

    let mut body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
    });
    if let Some(p) = params {
        body["params"] = p;
    }

    let line = serde_json::to_string(&body).map_err(|e| format!("json serialize: {e}"))?;
    {
        let mut stdin = session.stdin.lock().expect("lock");
        writeln!(&mut stdin, "{}", line).map_err(|e| format!("write: {e}"))?;
        stdin.flush().map_err(|e| format!("flush: {e}"))?;
    }

    let mut stdout = session.stdout.lock().expect("lock");
    let mut buf = String::new();
    stdout.read_line(&mut buf).map_err(|e| format!("read: {e}"))?;

    let v: serde_json::Value =
        serde_json::from_str(buf.trim()).map_err(|e| format!("json parse: {e}"))?;

    if let Some(err) = v.get("error") {
        return Err(err.get("message").and_then(|m| m.as_str()).unwrap_or("MCP error").to_string());
    }

    Ok(v["result"].clone())
}

/// Spawn an MCP server, run initialize handshake, return session + tool definitions.
fn initialize_mcp_session(config: &McpServerConfig) -> Result<(Arc<McpSession>, Vec<McpToolDef>), String> {
    let mut cmd = Command::new(&config.command);
    cmd.args(&config.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    for (k, v) in &config.env {
        cmd.env(k, v);
    }

    let mut child = cmd.spawn().map_err(|e| format!("spawn {}: {e}", &config.command))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;

    let session = Arc::new(McpSession {
        child: Mutex::new(child),
        stdin: Mutex::new(Box::new(stdin)),
        stdout: Mutex::new(BufReader::new(Box::new(stdout) as Box<dyn std::io::Read + Send>)),
        id_counter: Mutex::new(0),
    });

    // Initialize handshake
    let _init = rpc_call(&session, "initialize", Some(serde_json::json!({
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": {"name": "dsx", "version": "4.1.0"}
    })))?;

    // Send initialized notification
    {
        let msg = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let line = serde_json::to_string(&msg).expect("serialize MCP notification");
        let mut stdin = session.stdin.lock().expect("lock");
        writeln!(&mut stdin, "{}", line).map_err(|e| format!("write: {e}"))?;
        stdin.flush().map_err(|e| format!("flush: {e}"))?;
    }

    // Get tool list
    let tools_response = rpc_call(&session, "tools/list", None)?;
    let tools_array = tools_response.get("tools")
        .and_then(|t| t.as_array())
        .ok_or("tools/list response missing 'tools' array")?;

    let mut tool_defs = Vec::new();
    for t in tools_array {
        let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
        let description = t.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
        let input_schema = t.get("inputSchema").cloned().unwrap_or(serde_json::json!({}));
        tool_defs.push(McpToolDef { name, description, input_schema });
    }

    Ok((session, tool_defs))
}

struct McpToolDef {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

/// The shared handler function for all MCP tools.
/// Looks up the session by tool name from the global registry.
fn mcp_tool_handler(ctx: ToolCallCtx) -> ToolResult {
    let guard = MCP_SESSIONS.lock().expect("lock");
    let sessions = match guard.as_ref() {
        Some(s) => s,
        None => return ToolResult {
            success: false,
            content: "[ERROR] MCP sessions not initialized".to_string(),
        },
    };
    let session = match sessions.get(&ctx.name) {
        Some(s) => s.clone(),
        None => return ToolResult {
            success: false,
            content: format!("[ERROR] MCP tool '{}' — session not found", ctx.name),
        },
    };
    drop(guard);

    match rpc_call(&session, "tools/call", Some(serde_json::json!({
        "name": &ctx.name,
        "arguments": ctx.args,
    }))) {
        Ok(result) => {
            let content = result.get("content")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<&str>>()
                        .join("\n")
                })
                .unwrap_or_default();
            ToolResult::ok(content)
        }
        Err(e) => ToolResult {
            success: false,
            content: format!("[ERROR] MCP tool '{}' failed: {}", ctx.name, e),
        },
    }
}

/// Leak a String into &'static str for ToolHandler.description.
fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Register all tools from MCP servers into the ToolManager.
/// Call once at startup after all config is loaded.
pub fn register_mcp_servers(
    mgr: &mut crate::ToolManager,
    servers: &[McpServerConfig],
) -> Result<(), String> {
    if servers.is_empty() {
        return Ok(());
    }

    let mut sessions: HashMap<String, Arc<McpSession>> = HashMap::new();

    for server_config in servers {
        let (session, tools) = initialize_mcp_session(server_config)?;

        for tool in tools {
            sessions.insert(tool.name.clone(), session.clone());

            mgr.register(ToolHandler {
                key: ToolKey::new(&tool.name, ""),
                description: leak_str(tool.description),
                input_schema: tool.input_schema,
                handler: mcp_tool_handler,
                safety: |_| SafetyVerdict::Allow,
                default_timeout: Duration::from_secs(60),
            });
        }
    }

    *MCP_SESSIONS.lock().expect("lock") = Some(sessions);
    Ok(())
}

/// Clean up all MCP server processes.
pub fn shutdown_mcp_servers() {
    if let Some(sessions) = MCP_SESSIONS.lock().expect("lock").take() {
        for (_, session) in sessions {
            let _ = session.child.lock().expect("lock").kill();
        }
    }
}
