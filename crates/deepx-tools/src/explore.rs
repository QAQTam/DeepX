//! Project exploration: architecture analysis and module graph.
//! Directory trees are handled by `list_dir`; explore focuses on
//! structural understanding — crate dependencies, public APIs, test coverage.

use std::sync::atomic::Ordering;

use crate::CANCEL;
use crate::{ToolCallCtx, ToolResult};
use crate::CURRENT_WORKSPACE;

// ── Handler ──

pub(super) fn handle_explore(ctx: ToolCallCtx) -> ToolResult {
    let default_path = {
        let ws = CURRENT_WORKSPACE.get().map(|s| s.as_str()).unwrap_or(".");
        if ws == "." || ws.is_empty() {
            std::fs::read_to_string(deepx_types::platform::workspace_path())
                .ok().filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| ".".into())
        } else {
            ws.to_string()
        }
    };
    let path = ctx.get_str("path").unwrap_or(&default_path);
    ToolResult::ok(exec_architecture(path))
}

fn exec_architecture(path: &str) -> String {
    let root = std::path::Path::new(path);
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => return format!("[ERROR] Cannot determine current directory: {e}"),
    };
    let abs = if root.is_absolute() { root.to_path_buf() } else { cwd.join(root) };
    let abs_str = abs.to_string_lossy().to_string();

    let is_rust = abs.join("Cargo.toml").exists();
    let is_go = abs.join("go.mod").exists();
    let markers: Vec<&str> = ["Cargo.toml", "package.json", "go.mod", "pyproject.toml"]
        .iter().filter(|m| abs.join(m).exists()).copied().collect();

    let mut out = format!("[ARCHITECTURE]\npath: {abs_str}\n");
    if !markers.is_empty() {
        out.push_str(&format!("type: {}\n", markers.join(", ")));
    }

    if is_rust {
        out.push_str(&architecture_rust(&abs));
    } else if is_go {
        out.push_str(&architecture_go(&abs));
    } else {
        out.push_str("[HINT] Unknown project type. Use list_dir to browse, read_file to inspect.\n");
    }

    out.push_str(&format!("\n── {} chars, {} .rs files ──", out.len(), count_rs_files(&abs)));
    out
}

// ── Rust architecture ──

fn architecture_rust(root: &std::path::Path) -> String {
    let mut out = String::new();

    // 1. Crate dependency graph
    let crate_deps = parse_workspace_deps(root);
    if !crate_deps.is_empty() {
        out.push_str("\n## Crate Graph\n");
        for (name, deps) in &crate_deps {
            if deps.is_empty() {
                out.push_str(&format!("  {name}\n"));
            } else {
                out.push_str(&format!("  {name} → {}\n", deps.join(" ")));
            }
        }
    }

    // 2. Entry points
    for e in ["src/main.rs", "src/lib.rs"] {
        if root.join(e).exists() {
            out.push_str(&format!("  entry: {e}\n"));
        }
    }

    // 3. Public API + test counts per file
    let mut per_file: Vec<(String, Vec<String>, usize, usize)> = Vec::new();
    for entry in walk_rs_files(root) {
        if CANCEL.load(Ordering::Relaxed) { break; }
        let Ok(content) = std::fs::read_to_string(&entry) else { continue };
        let relative = entry.strip_prefix(root).unwrap_or(&entry).display().to_string();
        let mut tests = 0usize;
        let mut pub_items: Vec<String> = Vec::new();

        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("#[test]") || t.starts_with("#[cfg(test)]") { tests += 1; }
            let sig = if t.starts_with("pub fn ") {
                t.trim_start_matches("pub fn ").split('(').next().map(|n| format!("fn {}", n.trim()))
            } else if t.starts_with("pub async fn ") {
                t.trim_start_matches("pub async fn ").split('(').next().map(|n| format!("async fn {}", n.trim()))
            } else if t.starts_with("pub struct ") {
                t.trim_start_matches("pub struct ").split(&[' ', '{', '<'][..]).next().map(|n| format!("struct {}", n.trim()))
            } else if t.starts_with("pub enum ") {
                t.trim_start_matches("pub enum ").split(&[' ', '{'][..]).next().map(|n| format!("enum {}", n.trim()))
            } else if t.starts_with("pub trait ") {
                t.trim_start_matches("pub trait ").split(&[' ', '{', '<'][..]).next().map(|n| format!("trait {}", n.trim()))
            } else if t.starts_with("pub mod ") {
                t.trim_start_matches("pub mod ").trim_end_matches(';').trim().split(' ').next().map(|n| format!("mod {}", n.trim()))
            } else { None };

            if let Some(s) = sig {
                if s.len() < 100 { pub_items.push(s); }
            }
        }

        if !pub_items.is_empty() || tests > 0 {
            let lines = content.lines().count();
            per_file.push((relative, pub_items, lines, tests));
        }
    }

    // Sort: crate-level files first, then by path
    per_file.sort_by(|a, b| a.0.cmp(&b.0));

    if !per_file.is_empty() {
        out.push_str("\n## Public API\n");
        let mut current_dir = String::new();

        for (rel, items, lines, tests) in &per_file {
            // Group header when directory changes
            if let Some(dir) = std::path::Path::new(rel).parent().map(|p| p.to_string_lossy().to_string()) {
                if dir != current_dir {
                    current_dir = dir.clone();
                    out.push_str(&format!("  {}/\n", dir));
                }
            }

            let mut entry = format!("    {} ({}L", rel, lines);
            if *tests > 0 { entry.push_str(&format!(", {} tests", tests)); }
            entry.push(')');

            if !items.is_empty() {
                let top: Vec<&str> = items.iter().take(6).map(|s| s.as_str()).collect();
                entry.push_str(&format!(": {}", top.join(", ")));
                if items.len() > 6 {
                    entry.push_str(&format!(" +{} more", items.len() - 6));
                }
            }
            out.push_str(&format!("{entry}\n"));

            if out.len() > 6000 { out.push_str("  ... truncated\n"); break; }
        }
    }

    out
}

// ── Cargo.toml workspace dependency parsing ──

fn parse_workspace_deps(root: &std::path::Path) -> Vec<(String, Vec<String>)> {
    let mut crates: Vec<(String, Vec<String>)> = Vec::new();

    if let Ok(ws) = std::fs::read_to_string(root.join("Cargo.toml")) {
        // Is this a workspace root?
        let is_workspace = ws.contains("[workspace]");
        if is_workspace {
            // Parse workspace members
            let mut dirs: Vec<std::path::PathBuf> = Vec::new();
            for line in ws.lines() {
                let t = line.trim();
                if (t.starts_with('"') || t.starts_with("crates")) && t.contains('/') {
                    let member = t.trim_matches(',').trim_matches('"').trim_matches('\'');
                    let member_path = root.join(member);
                    // Handle glob: "crates/*" or "crates/dsx*"
                    if member.contains('*') {
                        let (pattern_dir, _) = member.rsplit_once('/').unwrap_or((".", member));
                        let full_pattern = root.join(pattern_dir);
                        if let Some(parent) = full_pattern.parent() {
                            if let Ok(entries) = std::fs::read_dir(parent) {
                                for e in entries.flatten() {
                                    if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                                        dirs.push(e.path());
                                    }
                                }
                            }
                        }
                    } else if member_path.exists() && member_path.is_dir() {
                        dirs.push(member_path);
                    }
                }
            }
            for dir in &dirs {
                if let Some(entry) = parse_single_crate(dir) {
                    crates.push(entry);
                }
            }
        }
    }

    // Also scan subdirs for standalone Cargo.toml files
    if crates.is_empty() {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(e) = parse_single_crate(&path) {
                        crates.push(e);
                    }
                }
            }
        }
        if crates.is_empty() {
            if let Some(e) = parse_single_crate(root) {
                crates.push(e);
            }
        }
    }

    crates
}

fn parse_single_crate(dir: &std::path::Path) -> Option<(String, Vec<String>)> {
    let toml_path = dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&toml_path).ok()?;
    let name = dir.file_name()?.to_string_lossy().to_string();
    let mut deps: Vec<String> = Vec::new();

    // Parse [dependencies] section
    let mut in_deps = false;
    let mut in_ws_deps = false;
    for line in content.lines() {
        let t = line.trim();
        if t == "[dependencies]" || t == "[build-dependencies]" {
            in_deps = true;
            in_ws_deps = false;
            continue;
        } else if t == "[workspace.dependencies]" {
            in_ws_deps = true;
            in_deps = false;
            continue;
        } else if t.starts_with('[') && t.ends_with(']') {
            in_deps = false;
            in_ws_deps = false;
            continue;
        }
        if in_deps || in_ws_deps {
            if let Some(dep_name) = t.split('=').next().map(|n| n.trim().trim_matches('"').to_string()) {
                if !dep_name.is_empty() {
                    deps.push(dep_name);
                }
            }
        }
    }
    Some((name, deps))
}

// ── Go architecture ──

fn architecture_go(root: &std::path::Path) -> String {
    let go_mod = std::fs::read_to_string(root.join("go.mod")).unwrap_or_default();
    let module = go_mod.lines().find(|l| l.starts_with("module "))
        .map(|l| l.trim_start_matches("module ").trim().to_string())
        .unwrap_or_else(|| "?".into());

    let mut out = format!("## Go Module\n  module: {module}\n");
    out.push_str(&derive_go_graph_stripped(root));
    out
}

fn derive_go_graph_stripped(root: &std::path::Path) -> String {
    let mut out = String::new();
    let mut seen_packages: Vec<String> = Vec::new();
    for entry in walk_ext_files(root, ".go") {
        if CANCEL.load(Ordering::Relaxed) { break; }
        let Ok(content) = std::fs::read_to_string(&entry) else { continue };
        let relative = entry.strip_prefix(root).unwrap_or(&entry).display().to_string();
        let mut exports: Vec<String> = Vec::new();

        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("func ") {
                let name = t.trim_start_matches("func ").split('(').next().unwrap_or("").trim().to_string();
                if name.chars().next().map_or(false, |c| c.is_uppercase()) {
                    exports.push(name);
                }
            }
        }
        if !exports.is_empty() {
            let pkg = std::path::Path::new(&relative).parent()
                .map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            if !seen_packages.contains(&pkg) {
                seen_packages.push(pkg);
            }
            out.push_str(&format!("  {}: {}\n", relative, exports.join(", ")));
        }
        if out.len() > 4000 { out.push_str("  ... truncated\n"); break; }
    }
    out
}

// ── File walkers ──

fn walk_rs_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with('.') || name == "target" || name == "node_modules" { continue; }
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                files.extend(walk_rs_files(&path));
            } else if name.ends_with(".rs") {
                files.push(path);
            }
        }
    }
    files.sort_by_key(|p| p.to_string_lossy().to_string());
    files
}

fn walk_ext_files(dir: &std::path::Path, ext: &str) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with('.') || name == "target" || name == "node_modules" || name == "vendor" { continue; }
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                files.extend(walk_ext_files(&path, ext));
            } else if name.ends_with(ext) {
                files.push(path);
            }
        }
    }
    files.sort_by_key(|p| p.to_string_lossy().to_string());
    files
}

fn count_rs_files(dir: &std::path::Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with('.') || name == "target" || name == "node_modules" { continue; }
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                count += count_rs_files(&path);
            } else if name.ends_with(".rs") {
                count += 1;
            }
        }
    }
    count
}

// ── Registration ──

use crate::{ToolHandler, ToolKey, SafetyVerdict};
use std::time::Duration;

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("explore", "scan"),
        description: "Analyze project architecture: crate dependencies, public API, entry points, test coverage. Call FIRST to understand project structure. For directory listing, use list_dir.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Project root directory", "default": "."}
            },
            "required": [],
            "additionalProperties": false
        }),
        handler: handle_explore,
        safety: |_| SafetyVerdict::allowed(),
        default_timeout: Duration::from_secs(30),
    });
}
