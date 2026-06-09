use crate::{parse_arg, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

// ── move_file ──

pub(super) fn exec_move_file(args: &str) -> String {
    let source = parse_arg(args, "source");
    let dest = parse_arg(args, "dest");
    if let Some(parent) = std::path::Path::new(&dest).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::rename(&source, &dest) {
        Ok(_) => format!("[OK] Moved {} → {}", source, dest),
        Err(e) => format!("[ERROR] Cannot move {}: {}\n[HINT] Check source exists and target directory is writable.", source, e),
    }
}

handler!(handle_move_file, exec_move_file);

// ── copy_file ──

pub(super) fn exec_copy_file(args: &str) -> String {
    let source = parse_arg(args, "source");
    let dest = parse_arg(args, "dest");
    if let Some(parent) = std::path::Path::new(&dest).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::copy(&source, &dest) {
        Ok(size) => format!("[OK] Copied {} → {} ({} bytes)", source, dest, size),
        Err(e) => format!("[ERROR] Cannot copy {}: {}\n[HINT] Check source exists and target directory is writable.", source, e),
    }
}

handler!(handle_copy_file, exec_copy_file);

// ── Registration ──


pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("move_file", ""),
        description: "Move or rename a file or directory. Creates parent dirs of dest.",
        input_schema: serde_json::json!({"type":"object","properties":{"source":{"type":"string","description":"Source path"},"dest":{"type":"string","description":"Destination path"}},"required":["source","dest"],"additionalProperties":false}),
        handler: handle_move_file,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("copy_file", ""),
        description: "Copy a file. Creates parent dirs of dest.",
        input_schema: serde_json::json!({"type":"object","properties":{"source":{"type":"string","description":"Source path"},"dest":{"type":"string","description":"Destination path"}},"required":["source","dest"],"additionalProperties":false}),
        handler: handle_copy_file,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
