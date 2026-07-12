//! Context7 documentation queries — v2 API.
//!   https://context7.com/docs/api-guide
//!
//! Library ID format (from docs): `/source/name`
//!   GitHub:   /vercel/next.js   or   /vercel/next.js@v15.1.8
//!   Website:  /websites/uploadcare_com
//!   llms.txt: /llmstxt/<source>
//!   npm:      /npm/<name>

use crate::{JsonArgs, ToolCallCtx, ToolResult, ToolHandler, ToolRisk};
use std::sync::OnceLock;

static C7_KEY: OnceLock<String> = OnceLock::new();

pub fn set_c7_key(key: &str) { let _ = C7_KEY.set(key.to_string()); }

fn c7_key() -> String {
    C7_KEY.get().cloned().or_else(|| std::env::var("CONTEXT7_API_KEY").ok()).unwrap_or_default()
}

// ── URL encoding ──

fn urlenc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            b' ' => o.push_str("%20"),
            _ => { o.push('%'); o.push_str(&format!("{:02X}", b)); }
        }
    }
    o
}

/// Normalize a library ID: ensure leading `/` per API spec.
fn norm_id(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() { return s.to_string(); }
    if s.starts_with('/') { s.to_string() } else { format!("/{s}") }
}

// ── HTTP helper ──

/// GET /api/v2{path}, return body on success (200). On non-200, parse error
/// JSON from body for a better message. On 301, include redirectUrl in error.
fn c7_get(path: &str) -> Result<String, String> {
    let resp = ureq::get(&format!("https://context7.com/api/v2{path}"))
        .header("Authorization", &format!("Bearer {}", c7_key()))
        .call().map_err(|e| format!("request: {e}"))?;

    let status = resp.status();
    let body = resp.into_body().read_to_string().map_err(|e| format!("read: {e}"))?;

    if status == 200 {
        return Ok(body);
    }

    // Try to extract a friendlier message from the JSON error body.
    let default_err = || format!("HTTP {status}");
    let err = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(v) => {
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
            let code = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
            if status == 301 {
                let redir = v.get("redirectUrl").and_then(|r| r.as_str()).unwrap_or("");
                if !redir.is_empty() {
                    return Err(format!("REDIRECT:{redir}"));
                }
            }
            if msg.is_empty() && code.is_empty() { default_err() }
            else if !code.is_empty() && !msg.is_empty() { format!("HTTP {status} [{code}]: {msg}") }
            else { format!("HTTP {status}: {}{}", msg, code) }
        }
        Err(_) => default_err(),
    };
    Err(err)
}

/// GET with up to one redirect retry (library moved / renamed).
fn c7_get_retry(path: &str, allow_redirect: bool) -> Result<String, String> {
    match c7_get(path) {
        Ok(body) => Ok(body),
        Err(e) if allow_redirect && e.starts_with("REDIRECT:") => {
            let new_id = &e["REDIRECT:".len()..];
            // Reconstruct URL with the new library ID.
            let new_path = if let Some(q_pos) = path.find('?') {
                let base = &path[..q_pos];
                let qs   = &path[q_pos..];
                if base == "/context" {
                    format!("/context?libraryId={}{}", urlenc(new_id),
                        // drop old libraryId param, keep rest
                        if let Some(rest) = qs.find("&query=").or_else(|| qs.find("&topic=")) {
                            &qs[rest..]
                        } else { "" })
                } else if base == "/libs/search" {
                    format!("/libs/search?libraryName={}", urlenc(new_id))
                } else {
                    path.to_string()
                }
            } else {
                format!("/context?libraryId={}", urlenc(new_id))
            };
            c7_get(&new_path)
        }
        Err(e) => Err(e),
    }
}

// ── Handler: dispatch by params ──

pub(super) fn handle_c7(ctx: ToolCallCtx) -> ToolResult {
    let has_name = !ctx.args.s("name").is_empty();
    let has_lib  = !ctx.args.s("library_id").is_empty();

    if has_lib && !has_name {
        // Context mode: library_id + query
        ToolResult::ok(&c7_context(&ctx.args))
    } else {
        // Search mode: name [+ query]
        ToolResult::ok(&c7_search(&ctx.args))
    }
}

// ── Search (resolve library name → ID) ──

fn c7_search(args: &serde_json::Value) -> String {
    let name = args.s("name");
    if name.is_empty() {
        return crate::json_err("MISSING_NAME", "context7: 'name' required", "Library name to search.");
    }
    let q = args.s("query");
    let mut path = format!("/libs/search?libraryName={}", urlenc(&norm_id(&name)));
    if !q.is_empty() { path.push_str(&format!("&query={}", urlenc(&q))); }

    let body = match c7_get_retry(&path, true) {
        Ok(b) => b,
        Err(e) => return crate::json_err("C7_ERROR", &e, ""),
    };
    let data: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return crate::json_err("PARSE_ERROR", "Invalid JSON response", ""),
    };
    let arr = data.get("results").and_then(|v| v.as_array());
    match arr {
        Some(a) if !a.is_empty() => {
            let mut items: Vec<serde_json::Value> = Vec::new();
            for r in a.iter().take(5) {
                let id    = r.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let desc  = r.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let src   = r.get("source").and_then(|v| v.as_str()).unwrap_or("");
                let lang  = r.get("language").and_then(|v| v.as_str()).unwrap_or("");
                let stars = r.get("stars").and_then(|v| v.as_u64()).unwrap_or(0);
                let mut item = serde_json::json!({"id": id, "title": title, "description": desc});
                if !src.is_empty()  { item["source"]   = serde_json::Value::String(src.to_string()); }
                if !lang.is_empty() { item["language"] = serde_json::Value::String(lang.to_string()); }
                if stars > 0        { item["stars"]    = serde_json::Value::Number(stars.into()); }
                items.push(item);
            }
            let total = a.len();
            let mut result = serde_json::json!({"results": items, "count": total});
            if total > 5 { result["truncated"] = serde_json::Value::Bool(true); }
            crate::json_ok(result)
        }
        _ => crate::json_err("NOT_FOUND", &format!("No library matching: {name}"), "Try a different name or check context7.com."),
    }
}

// ── Context (query docs) ──

fn c7_context(args: &serde_json::Value) -> String {
    let lib = norm_id(&args.s("library_id"));
    let q   = args.s("query");
    if lib.is_empty() || lib == "/" {
        return crate::json_err("MISSING_ID", "context7: 'library_id' required", "Pass a library ID from search results.");
    }
    let path = format!("/context?libraryId={}&query={}&type=json", urlenc(&lib), urlenc(&q));
    let body = match c7_get_retry(&path, true) {
        Ok(b) => b,
        Err(e) => return crate::json_err("C7_ERROR", &e, ""),
    };
    let data: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return crate::json_err("PARSE_ERROR", "Invalid JSON response", ""),
    };

    let mut content = String::new();

    // ── Code snippets ──
    if let Some(snippets) = data.get("codeSnippets").and_then(|v| v.as_array()) {
        for (i, s) in snippets.iter().enumerate().take(5) {
            let title = s.get("codeTitle").and_then(|v| v.as_str()).unwrap_or("");
            let desc  = s.get("codeDescription").and_then(|v| v.as_str()).unwrap_or("");
            let lang  = s.get("codeLanguage").and_then(|v| v.as_str()).unwrap_or("");
            let file  = s.get("codeFile").and_then(|v| v.as_str()).unwrap_or("");

            if i > 0 { content.push_str("\n---\n"); }
            if !title.is_empty() {
                content.push_str(&format!("### {title}"));
                if !file.is_empty() { content.push_str(&format!("  — `{file}`")); }
                content.push('\n');
            } else if !file.is_empty() {
                content.push_str(&format!("### `{file}`\n"));
            }
            if !desc.is_empty() {
                content.push_str(&format!("{desc}\n\n"));
            }
            if let Some(list) = s.get("codeList").and_then(|v| v.as_array()) {
                for c in list.iter().take(3) {
                    if let Some(code) = c.get("code").and_then(|v| v.as_str()) {
                        let fence_lang = if lang.is_empty() { "" } else { lang };
                        content.push_str(&format!("```{fence_lang}\n{code}\n```\n"));
                    }
                }
            }
        }
    }

    // ── Info snippets (text docs) ──
    if let Some(infos) = data.get("infoSnippets").and_then(|v| v.as_array()) {
        if !infos.is_empty() {
            if !content.is_empty() { content.push_str("\n---\n"); }
            content.push_str("### Documentation\n");
            for s in infos.iter().take(5) {
                let bc = s.get("breadcrumb").and_then(|v| v.as_str()).unwrap_or("");
                let sc = s.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if !sc.is_empty() {
                    if !bc.is_empty() {
                        content.push_str(&format!("- **{bc}**: {sc}\n"));
                    } else {
                        content.push_str(&format!("- {sc}\n"));
                    }
                }
            }
        }
    }

    if content.is_empty() {
        return crate::json_err("NO_RESULTS", "No documentation found for this query.", "Try rephrasing or check context7.com.");
    }
    crate::json_ok(serde_json::json!({"content": content.trim()}))
}

// ── Registration ──

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler { key: "context7".to_string(),
        description: "Context7 docs: pass 'name' to search libraries → get ID. Pass 'library_id'+'query' to retrieve code snippets and info. Requires CONTEXT7_API_KEY.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "name":       {"type": "string", "description": "Library name to search (e.g. 'react', '/microsoft/windows-rs')"},
                "library_id": {"type": "string", "description": "Context7 library ID from search results (e.g. '/vercel/next.js')"},
                "query":      {"type": "string", "description": "Documentation question (e.g. 'How to use useState?')"}
            },
            "required": [],
            "additionalProperties": false
        }),
        handler: handle_c7, risk: ToolRisk::ReadOnly, default_timeout: std::time::Duration::from_secs(15),
    });
}
