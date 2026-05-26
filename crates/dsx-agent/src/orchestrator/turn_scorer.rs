/// Detect if an exec command duplicates an existing tool. Returns (tool_name, cmd_summary).
pub fn detect_tool_equivalent(cmd: &str) -> Option<(String, String)> {
    let cmd = cmd.trim();
    // cat/head/tail/bat/less → read_file
    if let Some(rest) = cmd.strip_prefix("cat ").or_else(|| cmd.strip_prefix("head "))
        .or_else(|| cmd.strip_prefix("tail ")).or_else(|| cmd.strip_prefix("bat "))
        .or_else(|| cmd.strip_prefix("less "))
    {
        let target = rest.trim().split_whitespace().next().unwrap_or("?");
        if target.contains('.') {
            return Some(("read_file".into(), format!("cat {}", target)));
        }
    }
    // sed -i → edit_file
    if cmd.starts_with("sed ") && cmd.contains(" -i") {
        let target = cmd.split_whitespace().last().unwrap_or("?");
        if target.contains('.') {
            return Some(("edit_file".into(), format!("sed -i ... {}", target)));
        }
    }
    // grep/rg → search
    if let Some(rest) = cmd.strip_prefix("grep ").or_else(|| cmd.strip_prefix("rg ")) {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 && parts.last().unwrap_or(&"").contains('.') {
            return Some(("search".into(), format!("grep {} {}", parts[0], parts.last().unwrap())));
        }
    }
    // tee file / >> file / > file → write_file (append)
    if cmd.contains("tee ") || cmd.contains(">> ") || cmd.contains("> ") {
        let pos = cmd.find("> ").or_else(|| cmd.find(">> ")).or_else(|| cmd.find("tee "));
        if let Some(p) = pos {
            let rest = &cmd[p..];
            let target = rest.split_whitespace().nth(1).unwrap_or("?");
            if target.contains('.') {
                return Some(("write_file".into(), format!("redirect → {}", target)));
            }
        }
    }
    None
}

// ── Path trust levels ──

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PathTrust {
    /// /tmp/, /var/tmp/ — no gate, no sandbox, write+exec freely
    Trusted,
    /// src/**/*.rs, Cargo.toml — full intent gate + sandbox
    HighStake,
}

pub fn classify_path(path: &str) -> PathTrust {
    let normalized = std::path::Path::new(path)
        .components()
        .collect::<std::path::PathBuf>()
        .to_string_lossy()
        .to_string();

    // For absolute paths, check directly.
    if normalized.starts_with('/') {
        if normalized.starts_with("/tmp/") || normalized.starts_with("/var/tmp/")
            || normalized.starts_with("/exam_sctrip/") || normalized.starts_with("/exam/")
        {
            return PathTrust::Trusted;
        }
    } else if normalized.starts_with("..") {
        // Parent-relative — resolve to absolute for trust check.
        if let Ok(abs) = std::path::absolute(&normalized) {
            let abs = abs.to_string_lossy();
            if abs.starts_with("/tmp/") || abs.starts_with("/var/tmp/") {
                return PathTrust::Trusted;
            }
        }
    }
    // Relative paths under the project dir are always HighStake.
    PathTrust::HighStake
}
