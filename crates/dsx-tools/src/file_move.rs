use crate::{parse_arg, ToolHandler, ToolKey, ToolCallCtx, ToolResult, SafetyVerdict, handler};

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

fn default_allow(_ctx: &ToolCallCtx) -> SafetyVerdict { SafetyVerdict::Allow }

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("move_file", ""),
        description: "Move or rename a file or directory. Creates parent dirs of dest.",
        input_schema: serde_json::json!({"type":"object","properties":{"source":{"type":"string","description":"Source path"},"dest":{"type":"string","description":"Destination path"}},"required":["source","dest"],"additionalProperties":false}),
        handler: handle_move_file,
        safety: default_allow,
        default_timeout: std::time::Duration::from_secs(30),
    });
}
