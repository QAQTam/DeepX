use crate::{parse_arg_or, ToolHandler, ToolKey, ToolCallCtx, ToolResult, SafetyVerdict, handler};

pub(super) fn exec_list_dir(args: &str) -> String {
    let path = parse_arg_or(args, "path", ".");
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            const MAX_LIST_DIR_ENTRIES: usize = 200;
            let mut result = String::from("Directory listing: ");
            result.push_str(&path);
            result.push('\n');
            let mut count = 0usize;
            let all: Vec<_> = entries.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).collect();
            let total = all.len();
            for entry in &all {
                if count >= MAX_LIST_DIR_ENTRIES { break; }
                count += 1;
                let ft = entry.file_type().map(|t| if t.is_dir() { "/" } else { "" }).unwrap_or("?");
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let name = entry.file_name();
                let name_s = name.to_string_lossy();
                if ft == "/" {
                    result.push_str(&format!("  {:<40} <DIR>\n", name_s + "/"));
                } else {
                    let sz = if size > 1024*1024 { format!("{:.1}M", size as f64 / 1_048_576.0) }
                        else if size > 1024 { format!("{}K", size / 1024) }
                        else { format!("{}B", size) };
                    result.push_str(&format!("  {:<40} {:>6}\n", name_s, sz));
                }
            }
            if total > MAX_LIST_DIR_ENTRIES {
                result.push_str(&format!("... [truncated: {} more entries]\n", total - MAX_LIST_DIR_ENTRIES));
            }
            result
        }
        Err(e) => format!("[ERROR] Cannot list {}: {}\n[HINT] Check if the directory exists and is readable.", path, e),
    }
}

handler!(handle_list_dir, exec_list_dir);

fn default_allow(_ctx: &ToolCallCtx) -> SafetyVerdict { SafetyVerdict::Allow }

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("list_dir", ""),
        description: "List files and directories with names and sizes.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Directory path","default":"."}},"additionalProperties":false}),
        handler: handle_list_dir,
        safety: default_allow,
        default_timeout: std::time::Duration::from_secs(15),
    });
}
