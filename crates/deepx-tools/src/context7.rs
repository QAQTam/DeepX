//! Context7 documentation queries — matches v2 API.

use crate::{JsonArgs, ToolCallCtx, ToolResult, ToolHandler, ToolRisk};
use std::sync::OnceLock;

static C7_KEY: OnceLock<String> = OnceLock::new();

pub fn set_c7_key(key: &str) { let _ = C7_KEY.set(key.to_string()); }

fn c7_key() -> String {
    C7_KEY.get().cloned().or_else(|| std::env::var("CONTEXT7_API_KEY").ok()).unwrap_or_default()
}

fn c7_get(path: &str) -> Result<String, String> {
    let resp = ureq::get(&format!("https://context7.com/api/v2{path}"))
        .header("Authorization", &format!("Bearer {}", c7_key()))
        .call().map_err(|e| format!("request: {e}"))?;
    if resp.status() != 200 { return Err(format!("HTTP {}", resp.status())); }
    resp.into_body().read_to_string().map_err(|e| format!("read: {e}"))
}

fn urlenc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() { match b { b'A'..=b'Z'|b'a'..=b'z'|b'0'..=b'9'|b'-'|b'_'|b'.'|b'~' => o.push(b as char), b' '=>o.push_str("%20"), _=>{o.push('%');o.push_str(&format!("{:02X}",b));} } }
    o
}

// ── Handler: dispatch by params ──

pub(super) fn handle_c7(ctx: ToolCallCtx) -> ToolResult {
    if !ctx.args.s("library_id").is_empty() {
        ToolResult::ok(&c7_context(&ctx.args))
    } else {
        ToolResult::ok(&c7_search(&ctx.args))
    }
}

// ── Search (resolve library name → ID) ──

fn c7_search(args: &serde_json::Value) -> String {
    let name = args.s("name");
    if name.is_empty() { return crate::json_err("MISSING_NAME", "context7: 'name' required", "Library name to search."); }
    let q = args.s("query");
    let mut path = format!("/libs/search?libraryName={}", urlenc(&name));
    if !q.is_empty() { path.push_str(&format!("&query={}", urlenc(&q))); }
    let body = match c7_get(&path) { Ok(b) => b, Err(e) => return crate::json_err("C7_ERROR", &e, ""), };
    let data: serde_json::Value = match serde_json::from_str(&body) { Ok(v) => v, Err(_) => return crate::json_err("PARSE_ERROR", "Invalid response", ""), };
    let arr = data.get("results").and_then(|v| v.as_array());
    match arr {
        Some(a) if !a.is_empty() => {
            let mut items: Vec<serde_json::Value> = Vec::new();
            for r in a.iter().take(5) {
                let id = r.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let desc = r.get("description").and_then(|v| v.as_str()).unwrap_or("");
                items.push(serde_json::json!({"id":id,"title":title,"description":desc}));
            }
            crate::json_ok(serde_json::json!({"results":items}))
        }
        _ => crate::json_err("NOT_FOUND", &format!("No library: {name}"), "Try a different name."),
    }
}

// ── Context (query docs) ──

fn c7_context(args: &serde_json::Value) -> String {
    let lib = args.s("library_id");
    let q = args.s("query");
    if lib.is_empty() { return crate::json_err("MISSING_ID", "context7: 'library_id' required", ""); }
    let path = format!("/context?libraryId={}&query={}&type=json", urlenc(&lib), urlenc(&q));
    let body = match c7_get(&path) { Ok(b) => b, Err(e) => return crate::json_err("C7_ERROR", &e, ""), };
    let data: serde_json::Value = match serde_json::from_str(&body) { Ok(v) => v, Err(_) => return crate::json_err("PARSE_ERROR", "Invalid response", ""), };

    let mut content = String::new();
    if let Some(snippets) = data.get("codeSnippets").and_then(|v| v.as_array()) {
        for s in snippets.iter().take(5) {
            let title = s.get("codeTitle").and_then(|v| v.as_str()).unwrap_or("");
            let desc = s.get("codeDescription").and_then(|v| v.as_str()).unwrap_or("");
            let lang = s.get("codeLanguage").and_then(|v| v.as_str()).unwrap_or("");
            content.push_str(&format!("## {title}\n{desc}\n[{lang}]\n"));
            if let Some(list) = s.get("codeList").and_then(|v| v.as_array()) {
                for c in list.iter().take(2) {
                    if let Some(code) = c.get("code").and_then(|v| v.as_str()) {
                        content.push_str(code); content.push('\n');
                    }
                }
            }
        }
    }
    if let Some(infos) = data.get("infoSnippets").and_then(|v| v.as_array()) {
        for s in infos.iter().take(3) {
            let bc = s.get("breadcrumb").and_then(|v| v.as_str()).unwrap_or("");
            let sc = s.get("content").and_then(|v| v.as_str()).unwrap_or("");
            content.push_str(&format!("{bc}: {sc}\n"));
        }
    }
    if content.is_empty() { return crate::json_err("NO_RESULTS", "No documentation found", ""); }
    crate::json_ok(serde_json::json!({"content": content.trim()}))
}

// ── Registration ──

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler { key: "context7".to_string(),
        description: "Context7 docs: pass 'name' to search libraries → get ID. Pass 'library_id'+'query' to retrieve code snippets and info. Requires CONTEXT7_API_KEY.",
        input_schema: serde_json::json!({"type":"object","properties":{"name":{"type":"string","description":"Library name to search"},"library_id":{"type":"string","description":"Context7 library ID (from search)"},"query":{"type":"string","description":"Documentation question"}},"required":[],"additionalProperties":false}),
        handler: handle_c7, risk: ToolRisk::ReadOnly, default_timeout: std::time::Duration::from_secs(15),
    });
}
