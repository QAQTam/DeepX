use std::time::{SystemTime, UNIX_EPOCH};

use crate::{parse_arg, ToolHandler, ToolKey, ToolCallCtx, ToolResult, handler};

pub(super) fn exec_delete_file(args: &str) -> String {
    let path = parse_arg(args, "path");
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return format!("[ERROR] {} does not exist.", path);
    }

    let trash_root = find_trash_root();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    // Preserve relative directory structure in trash
    let rel = if path.starts_with('/') {
        path.trim_start_matches('/').to_string()
    } else {
        path.to_string()
    };
    let safe_name = rel.replace('/', "__");
    let trash_path = trash_root.join(format!("{}.{}", safe_name, ts));

    if let Some(parent) = trash_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    match std::fs::rename(p, &trash_path) {
        Ok(_) => format!(
            "[OK] Moved to trash: .deepx-trash/{}\n[HINT] Restore with exec(\"mv {}\" \"{}\") or exec(\"ls .deepx-trash/\") to list trash.",
            trash_path.file_name().unwrap_or_default().to_string_lossy(),
            trash_path.display(), path
        ),
        Err(_e) => {
            // Cross-device rename fails — for files: copy+delete; for dirs: not supported
            if p.is_dir() {
                format!("[ERROR] Cannot trash directory across devices: {}\n[HINT] Use exec(\"rm -rf {}\") for cross-device deletion.", path, path)
            } else if let Err(e2) = std::fs::copy(p, &trash_path) {
                format!("[ERROR] Cannot trash {}: copy failed: {}\n[HINT] Check permissions and disk space.", path, e2)
            } else {
                match std::fs::remove_file(p) {
                    Ok(_) => format!(
                        "[OK] Moved to trash (cross-device): .deepx-trash/{}\n[HINT] Restore with exec(\"cp {}\" \"{}\").",
                        trash_path.file_name().unwrap_or_default().to_string_lossy(),
                        trash_path.display(), path
                    ),
                    Err(e2) => format!(
                        "[OK] Copied to trash but could not remove original: {}\n[HINT] The original file still exists at {}.", e2, path
                    ),
                }
            }
        }
    }
}

fn find_trash_root() -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    // Walk up to find project root (where .git or Cargo.toml exists)
    let mut current = cwd.as_path();
    loop {
        if current.join(".git").exists() || current.join("Cargo.toml").exists() {
            return current.join(".deepx-trash");
        }
        match current.parent() {
            Some(p) => current = p,
            None => return cwd.join(".deepx-trash"),
        }
    }
}

handler!(handle_delete_file, exec_delete_file);


pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: ToolKey::new("delete_file", ""),
        description: "Move a file or directory to trash (.deepx-trash/) instead of permanent deletion. Use restore_file to recover.",
        input_schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"File path to delete"}},"required":["path"],"additionalProperties":false}),
        handler: handle_delete_file,
        safety: crate::default_allow,
        default_timeout: std::time::Duration::from_secs(15),
    });
}
