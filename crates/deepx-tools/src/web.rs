//! Web tool — fetch URLs and search the web (Bing RSS).

use crate::{JsonArgs, ToolCallCtx, ToolResult, ToolHandler, ToolRisk};

pub(super) fn handle_web(ctx: ToolCallCtx) -> ToolResult {
    if ctx.args.s("url").starts_with("http") {
        ToolResult::ok(&web_fetch(&ctx.args))
    } else {
        ToolResult::ok(&web_search(&ctx.args))
    }
}

fn web_fetch(args: &serde_json::Value) -> String {
    let url = args.s("url");
    if url.is_empty() || !url.starts_with("http") {
        return crate::json_err("INVALID_URL", "web: url must start with http", "");
    }
    let resp = match ureq::get(&url).header("User-Agent", "Mozilla/5.0 (compatible; DeepX/0.7)").call() {
        Ok(r) => r, Err(e) => return crate::json_err("FETCH_ERROR", &format!("{e}"), ""),
    };
    let is_html = resp.headers().get("Content-Type").and_then(|v| v.to_str().ok()).map(|s| s.contains("html")).unwrap_or(false);
    let body = match resp.into_body().read_to_string() {
        Ok(b) => b, Err(_) => return crate::json_err("READ_ERROR", "read failed", ""),
    };
    let readable = if is_html || body.trim_start().starts_with("<") {
        html2text::from_read(body.as_bytes(), body.len().min(120_000)).unwrap_or(body)
    } else { body };
    if let Some(out) = args.get("output").and_then(|v| v.as_str()) {
        let _ = std::fs::write(crate::resolve_workspace_path(out), &readable);
    }
    crate::json_ok(serde_json::json!({"content": readable}))
}

const BING: &str = "https://cn.bing.com/search?format=rss&q=";

fn web_search(args: &serde_json::Value) -> String {
    let q = args.s("query");
    if q.is_empty() { return crate::json_err("MISSING_QUERY", "web: 'query' or 'url' required", ""); }
    let resp = match ureq::get(&format!("{BING}{}", urlenc(&q))).header("User-Agent", "Mozilla/5.0 (compatible; DeepX/0.7)").call() {
        Ok(r) => r, Err(e) => return crate::json_err("BING_ERROR", &format!("{e}"), ""),
    };
    let body = match resp.into_body().read_to_string() {
        Ok(b) => b, Err(_) => return crate::json_err("BING_ERROR", "read failed", ""),
    };
    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut pos = 0;
    while let Some(s) = body[pos..].find("<item>") {
        pos += s; let start = pos;
        if let Some(e) = body[pos..].find("</item>") { pos += e + 7; } else { break; }
        let xml = &body[start..pos];
        let t = xml_tag(xml, "title"); let l = xml_tag(xml, "link"); let sn = strip_html(&xml_tag(xml, "description"));
        if !t.is_empty() && !l.is_empty() { results.push(serde_json::json!({"title":t,"url":l,"snippet":sn})); if results.len() >= 10 { break; } }
    }
    if results.is_empty() { return crate::json_ok(serde_json::json!({"query":q,"results":[],"source":"bing"})); }
    crate::json_ok(serde_json::json!({"query":q,"results":results,"source":"bing"}))
}

fn urlenc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() { match b { b'A'..=b'Z'|b'a'..=b'z'|b'0'..=b'9'|b'-'|b'_'|b'.'|b'~' => o.push(b as char), b' '=>o.push_str("%20"), _=>{o.push('%');o.push_str(&format!("{:02X}",b));} } }
    o
}
fn xml_tag(xml: &str, tag: &str) -> String {
    let o = format!("<{}>", tag); let c = format!("</{}>", tag);
    if let (Some(s), Some(e)) = (xml.find(&o), xml.find(&c)) { xml[s+o.len()..e].to_string() } else { String::new() }
}
fn strip_html(s: &str) -> String {
    let mut o = String::new(); let mut t = false;
    for c in s.chars() { match c { '<'=>t=true, '>'=>t=false, _ if !t=>o.push(c), _=>{} } }
    o.replace("&amp;","&").replace("&lt;","<").replace("&gt;",">").replace("&quot;","\"").replace("&#39;","'")
}

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler { key: "web".to_string(),
        description: "Web operations: fetch URL content (pass 'url') or search the web via Bing RSS (pass 'query').",
        input_schema: serde_json::json!({"type":"object","properties":{"url":{"type":"string","description":"URL to fetch"},"query":{"type":"string","description":"Search query"},"output":{"type":"string","description":"Optional file path"}},"required":[],"additionalProperties":false}),
        handler: handle_web, risk: ToolRisk::ReadOnly, default_timeout: std::time::Duration::from_secs(30),
    });
}
