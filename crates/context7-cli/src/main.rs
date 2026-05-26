//! context7-cli — standalone Context7 documentation lookup tool.
//!
//! Usage:
//!   context7-cli search <library-name> [query]    → find libraries
//!   context7-cli query <library-id> <question>     → get docs by ID
//!   context7-cli <library-name> <question>         → search + query in one step
//!   context7-cli --json <...>                       → raw JSON output
//!
//! Requires CONTEXT7_API_KEY env var.
//! Get a key at https://context7.com/dashboard

use std::env;

const BASE: &str = "https://context7.com/api/v2";

fn api_key() -> Result<String, String> {
    env::var("CONTEXT7_API_KEY").map_err(|_| {
        "CONTEXT7_API_KEY not set.\n\
         Get your key at https://context7.com/dashboard\n\
         Then: export CONTEXT7_API_KEY=ctx7sk-..."
            .into()
    })
}

fn get(path: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("context7-cli/2.0")
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(format!("{BASE}{path}"))
        .header("Authorization", format!("Bearer {}", api_key()?))
        .send()
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    let text = resp.text().map_err(|e| format!("read response: {e}"))?;

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), text));
    }
    Ok(text)
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
            out.push(c);
        } else if c == ' ' {
            out.push('+');
        } else {
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf);
            for b in encoded.bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// Search libraries by name + optional query.
/// Returns JSON array of {id, title, description, ...}
fn search(name: &str, query: &str, json_out: bool) -> Result<String, String> {
    let mut path = format!("/libs/search?libraryName={}", urlencode(name));
    if !query.is_empty() {
        path.push_str(&format!("&query={}", urlencode(query)));
    }
    let resp = get(&path)?;
    if !json_out {
        let v: serde_json::Value = serde_json::from_str(&resp).map_err(|e| e.to_string())?;
        let results = v.get("results").and_then(|r| r.as_array());
        match results {
            Some(arr) if !arr.is_empty() => {
                let mut out = String::new();
                for r in arr.iter().take(10) {
                    let id = r.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let desc = r.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    let stars = r.get("stars").and_then(|v| v.as_u64()).unwrap_or(0);
                    let score = r.get("benchmarkScore").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    out.push_str(&format!(
                        "  {id}\n  {title}  ★{stars}  score:{score:.1}\n  {desc}\n\n"
                    ));
                }
                Ok(out)
            }
            _ => Ok(format!("No results for '{name}'")),
        }
    } else {
        Ok(resp)
    }
}

/// Query docs for a library ID.
/// Returns JSON with codeSnippets and infoSnippets.
fn query(library_id: &str, question: &str, json_out: bool) -> Result<String, String> {
    let path = format!(
        "/context?libraryId={}&query={}&type=json",
        urlencode(library_id),
        urlencode(question)
    );
    let resp = get(&path)?;

    if json_out {
        return Ok(resp);
    }

    let v: serde_json::Value = serde_json::from_str(&resp).map_err(|e| e.to_string())?;

    let mut out = String::new();

    // Code snippets
    if let Some(snippets) = v.get("codeSnippets").and_then(|s| s.as_array()) {
        if !snippets.is_empty() {
            out.push_str("── Code ──\n\n");
            for s in snippets.iter().take(8) {
                let title = s.get("codeTitle").and_then(|v| v.as_str()).unwrap_or("");
                let desc = s.get("codeDescription").and_then(|v| v.as_str()).unwrap_or("");
                let lang = s.get("codeLanguage").and_then(|v| v.as_str()).unwrap_or("");
                let page = s.get("pageTitle").and_then(|v| v.as_str()).unwrap_or("");
                out.push_str(&format!("## {title}\n{desc}\n({page}) [{lang}]\n\n"));
                if let Some(list) = s.get("codeList").and_then(|l| l.as_array()) {
                    for c in list {
                        if let Some(code) = c.get("code").and_then(|v| v.as_str()) {
                            out.push_str("```");
                            out.push_str(lang);
                            out.push('\n');
                            out.push_str(code);
                            out.push_str("\n```\n\n");
                        }
                    }
                }
            }
        }
    }

    // Info snippets
    if let Some(snippets) = v.get("infoSnippets").and_then(|s| s.as_array()) {
        if !snippets.is_empty() {
            out.push_str("── Docs ──\n\n");
            for s in snippets.iter().take(5) {
                let bc = s.get("breadcrumb").and_then(|v| v.as_str()).unwrap_or("");
                let content = s.get("content").and_then(|v| v.as_str()).unwrap_or("");
                out.push_str(&format!("**{bc}**\n{content}\n\n"));
            }
        }
    }

    if out.is_empty() {
        Ok(format!("No results for '{question}' in {library_id}"))
    } else {
        Ok(out)
    }
}

/// Extract library ID from search response JSON.
fn first_lib_id(search_resp: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(search_resp).ok()?;
    v.get("results")?.as_array()?.first()?.get("id")?.as_str().map(|s| s.to_string())
}

fn main() -> Result<(), String> {
    let raw: Vec<String> = env::args().collect();
    let json_out = raw.iter().any(|a| a == "--json");
    let args: Vec<&str> = raw.iter().filter(|a| *a != "--json").map(|s| s.as_str()).collect();

    if args.len() < 3 {
        eprintln!("Usage:");
        eprintln!("  context7-cli search <library-name> [query]");
        eprintln!("  context7-cli query <library-id|library-name> <question>");
        eprintln!("  context7-cli <library-name> <question>           (auto search + query)");
        eprintln!("\nOptions:");
        eprintln!("  --json    output raw JSON");
        eprintln!("\nRequires CONTEXT7_API_KEY env var.");
        std::process::exit(1);
    }

    let result = match args[1] {
        "search" => search(args[2], "", json_out),
        "query" => {
            let target = args[2];
            let question = args.get(3).unwrap_or(&"");
            if question.is_empty() {
                return Err("usage: context7-cli query <library-id> <question>".into());
            }
            // If target is a library name (no /), search first
            let id = if target.starts_with('/') {
                target.to_string()
            } else {
                eprintln!("Resolving '{target}'...");
                let resp = get(&format!("/libs/search?libraryName={}", urlencode(target)))?;
                match first_lib_id(&resp) {
                    Some(id) => {
                        eprintln!("  → {id}");
                        id
                    }
                    None => return Err(format!("Library '{target}' not found")),
                }
            };
            query(&id, question, json_out)
        }
        _ => {
            // Shorthand: context7-cli <library> <question>
            let lib = args[1];
            let q = args.get(2).unwrap_or(&"");
            if q.is_empty() {
                return Err("usage: context7-cli <library> <question>".into());
            }
            eprintln!("Resolving '{lib}'...");
            let resp = get(&format!("/libs/search?libraryName={}", urlencode(lib)))?;
            match first_lib_id(&resp) {
                Some(id) => {
                    eprintln!("  → {id}\n");
                    query(&id, q, json_out)
                }
                None => Err(format!("Library '{lib}' not found")),
            }
        }
    };

    match result {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}
