//! Context7 documentation queries — v2 API.
//!   https://context7.com/docs/api-guide
//!
//! Library ID format (from docs): `/source/name`
//!   GitHub:   /vercel/next.js   or   /vercel/next.js@v15.1.8
//!   Website:  /websites/uploadcare_com
//!   llms.txt: /llmstxt/<source>
//!   npm:      /npm/<name>
//!
//! Pagination: pass `page` (1-based, default 1) and `per_page` (default 3, max 5).
//! Results are cached for 5 min keyed by (library_id, query), so successive
//! page requests avoid redundant API calls.

use crate::{JsonArgs, ToolCallCtx, ToolHandler, ToolResult, ToolRisk};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static C7_KEY: OnceLock<String> = OnceLock::new();

pub fn set_c7_key(key: &str) {
    let _ = C7_KEY.set(key.to_string());
}

fn c7_key() -> String {
    C7_KEY
        .get()
        .cloned()
        .or_else(|| std::env::var("CONTEXT7_API_KEY").ok())
        .unwrap_or_default()
}

// ── Response cache (keyed by library_id|query, TTL 5 min) ──

struct C7CacheEntry {
    json: serde_json::Value,
    at: Instant,
}

static C7_CACHE: OnceLock<Mutex<HashMap<String, C7CacheEntry>>> = OnceLock::new();

fn c7cache() -> &'static Mutex<HashMap<String, C7CacheEntry>> {
    C7_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn c7cache_get(key: &str) -> Option<serde_json::Value> {
    let cache = c7cache().lock().ok()?;
    cache
        .get(key)
        .filter(|e| e.at.elapsed() < Duration::from_secs(300))
        .map(|e| e.json.clone())
}

fn c7cache_set(key: String, json: serde_json::Value) {
    if let Ok(mut c) = c7cache().lock() {
        c.insert(
            key,
            C7CacheEntry {
                json,
                at: Instant::now(),
            },
        );
        // Prune stale entries occasionally
        if c.len() > 128 {
            c.retain(|_, v| v.at.elapsed() < Duration::from_secs(600));
        }
    }
}

// ── URL encoding ──

fn urlenc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                o.push(b as char)
            }
            b' ' => o.push_str("%20"),
            _ => {
                o.push('%');
                o.push_str(&format!("{:02X}", b));
            }
        }
    }
    o
}

/// Normalize a library ID: ensure leading `/` per API spec.
fn norm_id(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return s.to_string();
    }
    if s.starts_with('/') {
        s.to_string()
    } else {
        format!("/{s}")
    }
}

// ── HTTP helper ──

/// GET /api/v2{path}, return body on success (200). On non-200, parse error
/// JSON from body for a better message. On 301, include redirectUrl in error.
fn c7_get(path: &str) -> Result<String, String> {
    let resp = ureq::get(&format!("https://context7.com/api/v2{path}"))
        .header("Authorization", &format!("Bearer {}", c7_key()))
        .call()
        .map_err(|e| format!("request: {e}"))?;

    let status = resp.status();
    let body = resp
        .into_body()
        .read_to_string()
        .map_err(|e| format!("read: {e}"))?;

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
            if msg.is_empty() && code.is_empty() {
                default_err()
            } else if !code.is_empty() && !msg.is_empty() {
                format!("HTTP {status} [{code}]: {msg}")
            } else {
                format!("HTTP {status}: {}{}", msg, code)
            }
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
                let qs = &path[q_pos..];
                if base == "/context" {
                    format!(
                        "/context?libraryId={}{}",
                        urlenc(new_id),
                        // drop old libraryId param, keep rest
                        if let Some(rest) = qs.find("&query=").or_else(|| qs.find("&topic=")) {
                            &qs[rest..]
                        } else {
                            ""
                        }
                    )
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
    let has_lib = !ctx.args.s("library_id").is_empty();
    let has_q = !ctx.args.s("query").is_empty();

    if has_lib && has_q {
        // Context mode: user has a library ID + question → query docs directly
        ToolResult::ok(&c7_context(&ctx.args))
    } else if has_lib {
        // Library ID without query → do context with empty query (gets overview)
        ToolResult::ok(&c7_context(&ctx.args))
    } else if has_name {
        // Search mode: user needs to discover the library ID
        ToolResult::ok(&c7_search(&ctx.args))
    } else {
        ToolResult::ok(&crate::json_err(
            "MISSING_PARAMS",
            "context7 requires 'name' (to search) or 'library_id'+'query' (to query docs)",
            "Pass 'name' to find a library, then 'library_id'+'query' to retrieve docs.",
        ))
    }
}

// ── Search (resolve library name → ID) ──

fn c7_search(args: &serde_json::Value) -> String {
    let name = args.s("name");
    if name.is_empty() {
        return crate::json_err(
            "MISSING_NAME",
            "context7: 'name' required",
            "Library name to search.",
        );
    }
    let q = args.s("query");
    let mut path = format!("/libs/search?libraryName={}", urlenc(&norm_id(&name)));
    if !q.is_empty() {
        path.push_str(&format!("&query={}", urlenc(&q)));
    }

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
                let id = r.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let desc = r.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let src = r.get("source").and_then(|v| v.as_str()).unwrap_or("");
                let lang = r.get("language").and_then(|v| v.as_str()).unwrap_or("");
                let stars = r.get("stars").and_then(|v| v.as_u64()).unwrap_or(0);
                let mut item = serde_json::json!({"id": id, "title": title, "description": desc});
                if !src.is_empty() {
                    item["source"] = serde_json::Value::String(src.to_string());
                }
                if !lang.is_empty() {
                    item["language"] = serde_json::Value::String(lang.to_string());
                }
                if stars > 0 {
                    item["stars"] = serde_json::Value::Number(stars.into());
                }
                items.push(item);
            }
            let total = a.len();
            let mut result = serde_json::json!({"results": items, "count": total});
            if total > 5 {
                result["truncated"] = serde_json::Value::Bool(true);
            }
            crate::json_ok(result)
        }
        _ => crate::json_err(
            "NOT_FOUND",
            &format!("No library matching: {name}"),
            "Try a different name or check context7.com.",
        ),
    }
}

// ── Context (query docs, paginated) ──

/// Fetch + cache API response, then format one page of snippets.
fn c7_context(args: &serde_json::Value) -> String {
    let lib = norm_id(&args.s("library_id"));
    let q = args.s("query");
    if lib.is_empty() || lib == "/" {
        return crate::json_err(
            "MISSING_ID",
            "context7: 'library_id' required",
            "Pass a library ID from search results.",
        );
    }
    let query = if q.is_empty() { "overview" } else { &q };

    // Page params: 1-based, clamped.
    let page: usize = args
        .get("page")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .max(1) as usize;
    let per_page: usize = args
        .get("per_page")
        .and_then(|v| v.as_u64())
        .unwrap_or(3)
        .clamp(1, 5) as usize;

    // ── Fetch (cache-first) ──
    let cache_key = format!("{lib}|{query}");
    let data: serde_json::Value = if let Some(cached) = c7cache_get(&cache_key) {
        cached
    } else {
        let path = format!(
            "/context?libraryId={}&query={}&type=json",
            urlenc(&lib),
            urlenc(query)
        );
        let body = match c7_get_retry(&path, true) {
            Ok(b) => b,
            Err(e) => return crate::json_err("C7_ERROR", &e, ""),
        };
        let v: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => return crate::json_err("PARSE_ERROR", "Invalid JSON response", ""),
        };
        c7cache_set(cache_key, v.clone());
        v
    };

    // ── Flatten all snippets into a uniform numbered list ──
    #[derive(Clone)]
    struct FlatSnippet {
        kind: &'static str, // "code" | "info"
        title: String,
        body: String,
    }

    let mut all: Vec<FlatSnippet> = Vec::new();

    if let Some(arr) = data.get("codeSnippets").and_then(|v| v.as_array()) {
        for s in arr {
            let title = s.get("codeTitle").and_then(|v| v.as_str()).unwrap_or("");
            let desc = s
                .get("codeDescription")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let lang = s.get("codeLanguage").and_then(|v| v.as_str()).unwrap_or("");
            let file = s.get("codeFile").and_then(|v| v.as_str()).unwrap_or("");

            let mut body = String::new();
            // Header line
            if !title.is_empty() {
                body.push_str(&format!("**{title}**"));
                if !file.is_empty() {
                    body.push_str(&format!("  — `{file}`"));
                }
                if !lang.is_empty() {
                    body.push_str(&format!("  [{lang}]"));
                }
            } else if !file.is_empty() {
                body.push_str(&format!("**`{file}`**"));
                if !lang.is_empty() {
                    body.push_str(&format!("  [{lang}]"));
                }
            }
            if !desc.is_empty() {
                body.push('\n');
                body.push_str(desc);
            }
            if let Some(list) = s.get("codeList").and_then(|v| v.as_array()) {
                for c in list.iter().take(2) {
                    if let Some(code) = c.get("code").and_then(|v| v.as_str()) {
                        let fence_lang = if lang.is_empty() { "" } else { lang };
                        body.push_str(&format!("\n```{fence_lang}\n{code}\n```"));
                    }
                }
            }
            let title_clean = if title.is_empty() {
                file.to_string()
            } else {
                title.to_string()
            };
            all.push(FlatSnippet {
                kind: "code",
                title: title_clean,
                body,
            });
        }
    }

    if let Some(arr) = data.get("infoSnippets").and_then(|v| v.as_array()) {
        for s in arr {
            let bc = s.get("breadcrumb").and_then(|v| v.as_str()).unwrap_or("");
            let sc = s.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if !sc.is_empty() {
                let title = bc.to_string();
                let body = if !bc.is_empty() {
                    format!("{bc}: {sc}")
                } else {
                    sc.to_string()
                };
                all.push(FlatSnippet {
                    kind: "info",
                    title,
                    body,
                });
            }
        }
    }

    if all.is_empty() {
        return crate::json_err(
            "NO_RESULTS",
            "No documentation found for this query.",
            "Try rephrasing or check context7.com.",
        );
    }

    let total = all.len();
    let total_pages = (total + per_page - 1) / per_page;
    let page = page.min(total_pages); // clamp to last page
    let start = (page - 1) * per_page;
    let end = (start + per_page).min(total);
    let has_more = page < total_pages;

    // ── Build page content ──
    let mut content = String::new();

    // Pager header
    content.push_str(&format!(
        "─ Page {page}/{total_pages} · {per_page}/page · {total} snippets total ─\n\n"
    ));

    for (idx, s) in all[start..end].iter().enumerate() {
        let num = start + idx + 1;
        match s.kind {
            "code" => content.push_str(&format!("### {num}. 💻 {}\n{}\n\n", s.title, s.body)),
            _ => content.push_str(&format!("### {num}. 📄 {}\n{}\n\n", s.title, s.body)),
        }
    }

    // Pager footer
    if has_more {
        content.push_str(&format!(
            "─ Page {page}/{total_pages} · call again with `page={}` for snippets {}-{} ─\n",
            page + 1,
            end + 1,
            (end + per_page).min(total),
        ));
    } else {
        content.push_str(&format!("─ End of results ({total} snippets) ─\n"));
    }

    crate::json_ok(serde_json::json!({
        "content":     content.trim(),
        "page":        page,
        "per_page":    per_page,
        "total_pages": total_pages,
        "total":       total,
        "has_more":    has_more,
    }))
}

// ── Registration ──

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler { key: "context7".to_string(),
        description: "Context7 docs: pass 'name' to search libraries → get ID. Pass 'library_id'+'query' to retrieve code snippets and info (paginated via 'page'/'per_page'). Requires CONTEXT7_API_KEY.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "name":       {"type": "string",  "description": "Library name to search (e.g. 'react', '/microsoft/windows-rs')"},
                "library_id": {"type": "string",  "description": "Context7 library ID from search results (e.g. '/vercel/next.js')"},
                "query":      {"type": "string",  "description": "Documentation question (e.g. 'How to use useState?')"},
                "page":       {"type": "integer", "description": "Page number (1-based, default 1)", "minimum": 1},
                "per_page":   {"type": "integer", "description": "Snippets per page (1–5, default 3)", "minimum": 1, "maximum": 5}
            },
            "required": [],
            "additionalProperties": false
        }),
        handler: handle_c7, risk: ToolRisk::ReadOnly, default_timeout: std::time::Duration::from_secs(15),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_id_adds_slash() {
        assert_eq!(norm_id("microsoft/windows-rs"), "/microsoft/windows-rs");
        assert_eq!(norm_id("/vercel/next.js"), "/vercel/next.js");
        assert_eq!(norm_id(""), "");
        assert_eq!(norm_id("  /foo/bar  "), "/foo/bar");
    }

    #[test]
    fn urlenc_preserves_safe_chars() {
        let e = urlenc("abc123-_.~");
        assert_eq!(e, "abc123-_.~");
        assert_eq!(urlenc("hello world"), "hello%20world");
        assert_eq!(urlenc("/microsoft/windows-rs"), "%2Fmicrosoft%2Fwindows-rs");
    }

    #[test]
    fn search_finds_windows_rs() {
        let args = serde_json::json!({"name": "/microsoft/windows-rs"});
        let result = c7_search(&args);
        assert!(
            result.contains("\"status\":\"ok\""),
            "search failed: {result}"
        );
        assert!(
            result.contains("windows-rs"),
            "missing windows-rs in: {result}"
        );
    }

    #[test]
    fn search_with_norm_id() {
        let args = serde_json::json!({"name": "microsoft/windows-rs"});
        let result = c7_search(&args);
        assert!(
            result.contains("\"status\":\"ok\""),
            "search with bare name failed: {result}"
        );
    }

    #[test]
    fn search_missing_name() {
        let args = serde_json::json!({});
        let result = c7_search(&args);
        assert!(
            result.contains("MISSING_NAME"),
            "expected MISSING_NAME: {result}"
        );
    }

    #[test]
    fn context_page1_default() {
        let args = serde_json::json!({
            "library_id": "/microsoft/windows-rs",
            "query": "WinUI Microsoft.UI.Xaml"
        });
        let result = c7_context(&args);
        eprintln!("PAGE 1:\n{result}");
        assert!(
            result.contains("\"status\":\"ok\""),
            "page 1 failed: {result}"
        );
        assert!(result.contains("Page 1/"), "missing page header: {result}");
        assert!(
            result.contains("💻") || result.contains("📄"),
            "missing snippet markers: {result}"
        );
        // Verify pagination metadata
        assert!(
            result.contains("\"has_more\""),
            "missing has_more: {result}"
        );
        assert!(result.contains("\"total\""), "missing total: {result}");
    }

    #[test]
    fn context_page2_uses_cache() {
        // Page 1 primes the cache.
        let args1 = serde_json::json!({
            "library_id": "/microsoft/windows-rs",
            "query": "WinUI Microsoft.UI.Xaml",
            "per_page": 2
        });
        let r1 = c7_context(&args1);
        assert!(r1.contains("\"status\":\"ok\""), "page 1 failed: {r1}");

        // Page 2 should hit the cache (same key).
        let args2 = serde_json::json!({
            "library_id": "/microsoft/windows-rs",
            "query": "WinUI Microsoft.UI.Xaml",
            "page": 2,
            "per_page": 2
        });
        let r2 = c7_context(&args2);
        assert!(r2.contains("\"status\":\"ok\""), "page 2 failed: {r2}");
        assert!(r2.contains("Page 2/"), "expected Page 2/N header: {r2}");

        // Content should differ between pages.
        assert_ne!(r1, r2, "page 1 and 2 should return different snippets");
    }

    #[test]
    fn context_per_page_clamped() {
        let args = serde_json::json!({
            "library_id": "/microsoft/windows-rs",
            "query": "WinUI",
            "per_page": 100   // should clamp to 5
        });
        let r = c7_context(&args);
        assert!(
            r.contains("\"status\":\"ok\""),
            "clamped per_page failed: {r}"
        );
        // Should not contain more than 5 snippet markers.
        let count = r.matches("💻").count() + r.matches("📄").count();
        assert!(count <= 5, "too many snippets on page: {count}");
    }

    #[test]
    fn context_missing_id() {
        let args = serde_json::json!({"query": "something"});
        let result = c7_context(&args);
        assert!(
            result.contains("MISSING_ID"),
            "expected MISSING_ID: {result}"
        );
    }
}
