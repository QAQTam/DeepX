//! Web tools: search, fetch, Context7 documentation queries.

use crate::{JsonArgs, ToolCallCtx, ToolResult, ToolHandler, ToolRisk};

// Context7 REST API v2.
const C7_BASE: &str = "https://context7.com/api/v2";

use std::sync::OnceLock;

static C7_KEY: OnceLock<String> = OnceLock::new();

pub fn set_c7_key(key: &str) {
    let _ = C7_KEY.set(key.to_string());
}

fn c7_key() -> String {
    C7_KEY.get().cloned()
        .or_else(|| std::env::var("CONTEXT7_API_KEY").ok())
        .unwrap_or_default()
}

fn c7_get(path: &str) -> Result<String, String> {
    let resp = ureq::get(&format!("{C7_BASE}{path}"))
        .header("Authorization", &format!("Bearer {}", c7_key()))
            .header("User-Agent", "deepx/0.2")
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    let text = resp.into_body().read_to_string().map_err(|e| format!("read response: {e}"))?;
    if status != 200 {
        return Err(format!("HTTP {}: {}", status, text.chars().take(200).collect::<String>()));
    }
    Ok(text)
}

fn c7_url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
            out.push(c);
        } else if c == ' ' {
            out.push('+');
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

// ── Handler 函数 ──

pub(super) fn handle_fetch(ctx: ToolCallCtx) -> ToolResult {
    ToolResult::ok(exec_web_fetch(&ctx.args))
}

pub(super) fn handle_search(ctx: ToolCallCtx) -> ToolResult {
    ToolResult::ok(exec_web_search(&ctx.args))
}

pub(super) fn handle_c7_resolve(ctx: ToolCallCtx) -> ToolResult {
    ToolResult::ok(exec_context7_resolve(&ctx.args))
}

pub(super) fn handle_c7_query(ctx: ToolCallCtx) -> ToolResult {
    ToolResult::ok(exec_context7_query(&ctx.args))
}

// ── Context7 tools (REST API v2) ──

fn exec_context7_resolve(args: &serde_json::Value) -> String {
    let name = args.s("name");
    if name.is_empty() {
        return crate::json_err("MISSING_NAME", "context7_resolve: missing 'name'", "Provide the 'libraryName' parameter.");
    }
    let q = args.s_or("query", "");
    let mut path = format!("/libs/search?libraryName={}", c7_url_encode(&name));
    if !q.is_empty() {
        path.push_str(&format!("&query={}", c7_url_encode(&q)));
    }
    let resp = match c7_get(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("API_ERROR", &format!("Context7: {e}"), "Check the API key or network."),
    };
    let data: serde_json::Value = match serde_json::from_str(&resp) {
        Ok(d) => d,
        Err(e) => return crate::json_err("PARSE_ERROR", &format!("Context7 parse: {e}"), "The API returned an unexpected response."),
    };
    let results = match data.get("results").and_then(|r| r.as_array()) {
        Some(arr) if !arr.is_empty() => arr,
        _ => return crate::json_ok(serde_json::json!({"name": name, "content": format!("Context7: no results for '{}'", name)})),
    };
    let mut content = format!("Context7: {} results for '{}'\n\n", results.len(), name);
    for r in results.iter().take(8) {
        let id = r.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let desc = r.get("description").and_then(|v| v.as_str()).unwrap_or("");
        let score = r.get("benchmarkScore").and_then(|v| v.as_f64()).unwrap_or(0.0);
        content.push_str(&format!("  {id}  ({score:.1})\n  {title}\n  {desc}\n\n"));
    }
    crate::json_ok(serde_json::json!({"name": name, "results_count": results.len(), "content": content}))
}

fn exec_context7_query(args: &serde_json::Value) -> String {
    let library_id = args.s("library_id");
    if library_id.is_empty() {
        return crate::json_err("MISSING_LIBRARY_ID", "context7_query: missing 'library_id' parameter", "Provide the library ID obtained from context7_resolve.");
    }
    let q = args.s_or("query", "");
    let path = format!(
        "/context?libraryId={}&query={}&type=json",
        c7_url_encode(&library_id),
        c7_url_encode(&q)
    );
    let resp = match c7_get(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("API_ERROR", &format!("Context7: {e}"), "Check the API key or network."),
    };
    let data: serde_json::Value = match serde_json::from_str(&resp) {
        Ok(d) => d,
        Err(e) => return crate::json_err("PARSE_ERROR", &format!("Context7 parse: {e}"), "The API returned an unexpected response."),
    };

    let mut content = String::from("Context7:\n");

    if let Some(snippets) = data.get("codeSnippets").and_then(|s| s.as_array()) {
        for s in snippets.iter().take(5) {
            let title = s.get("codeTitle").and_then(|v| v.as_str()).unwrap_or("");
            let desc = s.get("codeDescription").and_then(|v| v.as_str()).unwrap_or("");
            let lang = s.get("codeLanguage").and_then(|v| v.as_str()).unwrap_or("");
            content.push_str(&format!("\n## {title}\n{desc}\n[{lang}]\n"));
            if let Some(list) = s.get("codeList").and_then(|l| l.as_array()) {
                for c in list.iter().take(2) {
                    if let Some(code) = c.get("code").and_then(|v| v.as_str()) {
                        if code.len() > 2000 {
                            let cut = code.char_indices().nth(2000).map(|(i, _)| i).unwrap_or(code.len());
                            content.push_str(&code[..cut]);
                            content.push_str("\n... [truncated]");
                        } else {
                            content.push_str(code);
                        }
                        content.push('\n');
                    }
                }
            }
        }
    }

    if let Some(snippets) = data.get("infoSnippets").and_then(|s| s.as_array()) {
        for s in snippets.iter().take(3) {
            let bc = s.get("breadcrumb").and_then(|v| v.as_str()).unwrap_or("");
            let snippet_content = s.get("content").and_then(|v| v.as_str()).unwrap_or("");
            content.push_str(&format!("\n  {bc}: {snippet_content}"));
        }
    }

    if content == "Context7:\n" {
        crate::json_ok(serde_json::json!({"library_id": library_id, "query": q, "content": format!("Context7: no results for '{}' in {}", q, library_id)}))
    } else {
        crate::json_ok(serde_json::json!({"library_id": library_id, "query": q, "content": content}))
    }
}

// ── Web fetch ──

fn exec_web_fetch(args: &serde_json::Value) -> String {
    let url = args.s("url");
    let url_lower = url.to_lowercase();
    if url_lower.starts_with("http://localhost")
        || url_lower.starts_with("https://localhost")
        || url_lower.starts_with("http://127.")
        || url_lower.starts_with("https://127.")
        || url_lower.starts_with("http://[::1]")
        || url_lower.starts_with("https://[::1]")
        || url_lower.starts_with("http://169.254.")
        || url_lower.starts_with("https://169.254.")
        || url_lower.starts_with("http://10.")
        || url_lower.starts_with("https://10.")
        || url_lower.starts_with("http://172.16.")
        || url_lower.starts_with("https://172.16.")
        || url_lower.starts_with("http://192.168.")
        || url_lower.starts_with("https://192.168.")
        || url.starts_with("file://")
    {
        return crate::json_err("LOCAL_URL", &format!("Cannot fetch internal/local URL: {}", url), "web_fetch only supports public URLs.");
    }
    let resp = match ureq::get(&url)
            .header("User-Agent", "deepx/0.2")
        .call()
    {
        Ok(r) => r,
        Err(e) => return crate::json_err("FETCH_FAILED", &format!("Cannot fetch {}: {}", url, e), "Check the URL or network."),
    };
    let status = resp.status();
    if let Some(len) = resp.headers().get("content-length").and_then(|h| h.to_str().ok()).and_then(|s| s.parse::<u64>().ok()) {
        if len > 5_000_000 {
            return crate::json_err("TOO_LARGE", &format!("Response too large: {} bytes > 5MB limit", len), "Use a URL that returns smaller content.");
        }
    }
    match resp.into_body().read_to_string() {
        Ok(body) => {
                    let readable = match html2text::from_read(body.as_bytes(), body.len().min(120_000)) {
                        Ok(t) => t,
                        Err(e) => return crate::json_err("HTML_PARSE_ERROR", &format!("html2text: {}", e), "The URL may not return HTML."),
                    };
                    let truncated = readable.len() > 100_000;
                    let display = if truncated {
                        let end = find_char_boundary(&readable, 100_000);
                        format!("{}... [truncated: {} total chars]", &readable[..end], readable.len())
                    } else { readable.clone() };
                    let output_path = args.s("output");
                    let saved = if !output_path.is_empty() { let path = &output_path;
                        if let Some(parent) = std::path::Path::new(path).parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        match std::fs::write(path, &readable) {
                            Ok(_) => format!("\nSaved to {}", path),
                            Err(e) => format!("\n[HINT] Could not save to {}: {}", path, e),
                        }
                    } else { String::new() };
                    let status_code = status.as_u16();
                    if status == 200 {
                        crate::json_ok(serde_json::json!({"url": url, "status": status_code, "content": format!("{} ({} chars)\n\n{}{}", status_code, display.len(), display, saved)}))
                    } else {
                        crate::json_ok(serde_json::json!({"url": url, "status": status_code, "content": format!("HTTP {}\n\n{}{}", status_code, display, saved)}))
                    }
                }
                Err(e) => crate::json_err("READ_FAILED", &format!("Cannot read response body: {}", e), "The URL may not return text."),
            }
    }

fn find_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() { return s.len(); }
    s.floor_char_boundary(max)
}

// ── Web search (BochaAI API) ──

const BOCHA_BASE: &str = "https://api.bocha.cn/v1";

static BOCHA_KEY: OnceLock<String> = OnceLock::new();

pub fn set_bocha_key(key: &str) {
    let _ = BOCHA_KEY.set(key.to_string());
}

fn bocha_key() -> String {
    BOCHA_KEY.get().cloned()
        .or_else(|| std::env::var("BOCHA_API_KEY").ok())
        .unwrap_or_default()
}

fn exec_web_search(args: &serde_json::Value) -> String {
    let query = args.s("query");
    if query.is_empty() {
        return crate::json_err("MISSING_QUERY", "web_search: missing 'query' parameter", "Provide a search query string.");
    }
    let api_key = bocha_key();
    if api_key.is_empty() {
        return crate::json_err("MISSING_API_KEY", "web_search: BOCHA_API_KEY not set", "Set the BOCHA_API_KEY environment variable or call set_bocha_key(). Get a free key at https://open.bochaai.com");
    }

    let body = serde_json::json!({
        "query": query,
        "summary": true,
        "count": 10,
    });

    let resp_text = match (|| -> Result<String, String> {
        let resp = ureq::post(&format!("{BOCHA_BASE}/web-search"))
            .header("Authorization", &format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("request: {e}"))?;
        let status = resp.status();
        let text = resp.into_body().read_to_string().map_err(|e| format!("read: {e}"))?;
        if status != 200 {
            return Err(format!("HTTP {}: {}", status, text.chars().take(300).collect::<String>()));
        }
        Ok(text)
    })() {
        Ok(b) => b,
        Err(e) => return crate::json_err("SEARCH_FAILED", &format!("Search failed: {}", e), "Check network or API key."),
    };

    let parsed: serde_json::Value = match serde_json::from_str(&resp_text) {
        Ok(v) => v,
        Err(e) => return crate::json_err("PARSE_ERROR", &format!("Failed to parse response: {e}"), "The API returned an unexpected response."),
    };

    let code = parsed.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
    if code != 200 {
        let msg = parsed.get("msg").and_then(|m| m.as_str()).unwrap_or("unknown");
        return crate::json_err("API_ERROR", &format!("Bocha API error (code {}): {}", code, msg), "The API returned an error.");
    }

    let results = parsed["data"]["webPages"]["value"]
        .as_array()
        .map(|arr| arr.clone())
        .unwrap_or_default();

    if results.is_empty() {
        return crate::json_ok(serde_json::json!({"query": query, "content": format!("Bocha: {}\n\n(no results)", query)}));
    }

    let mut content = format!("Bocha: {}\n", query);
    for r in results.iter() {
        let title = r.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let snippet = r.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
        let site = r.get("siteName").and_then(|v| v.as_str()).unwrap_or("");
        let date = r.get("datePublished")
            .or_else(|| r.get("dateLastCrawled"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        content.push_str(&format!("\n[{}]({})", title, url));
        if !snippet.is_empty() {
            content.push_str(&format!("\n  {}", snippet));
        }
        if !site.is_empty() || !date.is_empty() {
            let sep = if !site.is_empty() && !date.is_empty() { " · " } else { "" };
            content.push_str(&format!("\n  ({}{}{})", site, sep, date));
        }
        content.push('\n');
    }
    crate::json_ok(serde_json::json!({"query": query, "results_count": results.len(), "content": content}))
}

// ── 注册入口 ──

use std::time::Duration;

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: "web_fetch".to_string(),
        description: "Web operations: fetch, search, context7_resolve, context7_query. Use fetch to get URL content, search to query the web.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"},
                "output": {"type": "string", "description": "Optional file path to save the fetched content"}
            },
            "required": ["url"],
            "additionalProperties": false
        }),
        handler: handle_fetch,
        risk: ToolRisk::ReadOnly,
        default_timeout: Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: "web_search".to_string(),
        description: "Search the web",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query string"}
            },
            "required": ["query"],
            "additionalProperties": false
        }),
        handler: handle_search,
        risk: ToolRisk::ReadOnly,
        default_timeout: Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "web_context7_resolve".to_string(),
        description: "Resolve library name to Context7 ID",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Library name"},
                "query": {"type": "string", "description": "Optional filter to narrow results, e.g. 'hooks'"}
            },
            "required": ["name"],
            "additionalProperties": false
        }),
        handler: handle_c7_resolve,
        risk: ToolRisk::ReadOnly,
        default_timeout: Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "web_context7_query".to_string(),
        description: "Query Context7 docs",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "library_id": {"type": "string", "description": "Context7 library ID obtained from context7_resolve"},
                "query": {"type": "string", "description": "Documentation query, e.g. 'how to use useState'"}
            },
            "required": ["library_id"],
            "additionalProperties": false
        }),
        handler: handle_c7_query,
        risk: ToolRisk::ReadOnly,
        default_timeout: Duration::from_secs(15),
    });
}

