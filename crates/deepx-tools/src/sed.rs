//! sed tool — structured sed expression execution via deepx-sed.

use crate::{ToolCallCtx, ToolResult};
use deepx_sed::{RunConfig, errors::ScriptSource};

pub fn handle_sed(ctx: ToolCallCtx) -> ToolResult {
    let path = ctx.get_str("path").unwrap_or("");
    if path.is_empty() {
        return ToolResult { success: false, content: "[ERROR] sed: 'path' is required".into() };
    }

    let resolved = crate::resolve_workspace_path(path);

    let mut scripts: Vec<(String, Vec<u8>, ScriptSource)> = Vec::new();

    if let Some(expr) = ctx.get_str("expression") {
        if !expr.is_empty() {
            scripts.push((expr.to_string(), Vec::new(), ScriptSource::Expression(0)));
        }
    }
    if let Some(arr) = ctx.args.get("expressions").and_then(|v| v.as_array()) {
        for (i, v) in arr.iter().enumerate() {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    scripts.push((s.to_string(), Vec::new(), ScriptSource::Expression(i)));
                }
            }
        }
    }

    if scripts.is_empty() {
        return ToolResult { success: false, content: "[ERROR] sed: 'expression' or 'expressions' is required".into() };
    }

    let in_place = ctx.get_bool("in_place").unwrap_or(true);
    let quiet = ctx.get_bool("quiet").unwrap_or(false);
    let extended_regex = ctx.get_bool("extended_regex").unwrap_or(false);
    let dry_run = ctx.get_bool("dry_run").unwrap_or(true);

    // Read original
    let original = match std::fs::read_to_string(&resolved) {
        Ok(c) => c,
        Err(e) => return ToolResult {
            success: false,
            content: format!("[ERROR] sed: cannot read {}: {}", resolved, e),
        },
    };

    // For dry-run, work on a temp copy
    let target_path = if dry_run {
        let temp_dir = std::env::temp_dir();
        let name = std::path::Path::new(&resolved).file_name().unwrap_or_default().to_string_lossy();
        let tp = temp_dir.join(format!(".deepx_sed_{}", name));
        if let Err(e) = std::fs::write(&tp, &original) {
            return ToolResult {
                success: false,
                content: format!("[ERROR] sed: cannot create temp file: {}", e),
            };
        }
        tp.to_string_lossy().to_string()
    } else {
        resolved.clone()
    };

    let config = RunConfig {
        scripts_with_sources: scripts.clone(),
        input_files: vec![target_path.clone()],
        quiet,
        in_place: if dry_run { Some(String::new()) } else if in_place { Some(String::new()) } else { None },
        extended_regex,
        separate_files: false,
        line_length: 70,
        unbuffered: false,
        posix: false,
        strict_posix: false,
        follow_symlinks: false,
        sandbox: false,
        null_data: false,
        binary: false,
    };

    let expr_list: Vec<&str> = scripts.iter().map(|(s, _, _)| s.as_str()).collect();

    match deepx_sed::run(config) {
        Ok(_) => {
            if dry_run {
                let modified = std::fs::read_to_string(&target_path).unwrap_or_default();
                let _ = std::fs::remove_file(&target_path);

                let diff = similar::TextDiff::from_lines(&original, &modified);
                let diff_str = diff.unified_diff()
                    .context_radius(3)
                    .header(&format!("a/{}", path), &format!("b/{}", path))
                    .to_string();

                if diff_str.is_empty() {
                    ToolResult { success: true, content: format!("[DRY RUN] sed {} — no changes", path) }
                } else {
                    let (added, removed, _) = crate::file_shared::diff_stats(&diff_str);
                    ToolResult {
                        success: true,
                        content: format!("[DRY RUN] sed {} +{} -{} | {}\n\n{}",
                            path, added.max(1), removed.max(1), expr_list.join("; "), diff_str),
                    }
                }
            } else {
                // Real execution: read back modified file and show diff
                let modified = std::fs::read_to_string(&resolved).unwrap_or_default();
                let diff = similar::TextDiff::from_lines(&original, &modified);
                let diff_str = diff.unified_diff()
                    .context_radius(3)
                    .header(&format!("a/{}", path), &format!("b/{}", path))
                    .to_string();

                if diff_str.is_empty() {
                    ToolResult {
                        success: true,
                        content: format!("[OK] sed {} — no changes (expressions: {})", path, expr_list.join("; ")),
                    }
                } else {
                    let (added, removed, first_line) = crate::file_shared::diff_stats(&diff_str);
                    ToolResult {
                        success: true,
                        content: format!("[OK] sed {}:{} +{} -{} | {}\n\n{}",
                            path, first_line,
                            added.max(1), removed.max(1),
                            expr_list.join("; "),
                            diff_str.trim_end()),
                    }
                }
            }
        }
        Err(e) => {
            if dry_run { let _ = std::fs::remove_file(&target_path); }
            ToolResult { success: false, content: format!("[ERROR] sed {}: {}", path, e) }
        }
    }
}

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(crate::ToolHandler {
        key: crate::ToolKey::new("sed", ""),
        description: "Run sed expressions on a file. Use for find-replace (s/old/new/g), line deletion (/pattern/d), or line-range operations (1,5s/x/y/). Supports multiple chained expressions.",
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "expression": {"type": "string", "description": "A single sed expression, e.g. 's/old/new/g'"},
                "expressions": {"type": "array", "items": {"type": "string"}, "description": "Multiple sed expressions applied in order"},
                "in_place": {"type": "boolean", "description": "Edit file directly (default true)", "default": true},
                "quiet": {"type": "boolean", "description": "Suppress auto-print (-n)", "default": false},
                "extended_regex": {"type": "boolean", "description": "Use extended regex (-E)", "default": false},
                "dry_run": {"type": "boolean", "description": "Preview diff without modifying file", "default": true}
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        handler: handle_sed,
        risk: crate::ToolRisk::Write,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
