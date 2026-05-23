//! Memory tier and pitfall persistence.

/// Read a memory file, truncating to ~4K tokens (~15K chars) for context injection.
pub fn read_memory(seed: &str, tier: &str) -> String {
    let Some(path) = super::memory_path(seed, tier) else { return String::new(); };
    if !path.exists() { return String::new(); }
    let Ok(content) = std::fs::read_to_string(&path) else { return String::new(); };
    // Truncate to ~4K tokens (roughly 12K chars for CJK, 16K for ASCII).
    // Trim from the beginning to keep newest content.
    if content.len() > 16000 {
        let start = content.len() - 16000;
        let mut s = start;
        while s < content.len() && !content.is_char_boundary(s) { s += 1; }
        if let Some(nl) = content[s..].find('\n') {
            content[s + nl + 1..].to_string()
        } else {
            content[s..].to_string()
        }
    } else {
        content
    }
}

/// Overwrite a memory file with new content.
pub fn write_memory(seed: &str, tier: &str, content: &str) {
    let Some(path) = super::memory_path(seed, tier) else { return };
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(&path, content);
}

/// Append a learning entry to learning.md (cross-session self-improvement log).
pub fn append_learning(seed: &str, entry: &str) {
    let Some(path) = super::memory_path(seed, "learning") else { return };
    let _ = std::fs::create_dir_all(path.parent().unwrap());

    let mut existing = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };

    // Keep under ~12K chars (trim from beginning, keep newest)
    if existing.len() > 12000 {
        let cut = existing.len().saturating_sub(8000);
        let mut s = cut;
        while s < existing.len() && !existing.is_char_boundary(s) { s += 1; }
        if let Some(nl) = existing[s..].find('\n') {
            existing = existing[s + nl + 1..].to_string();
        } else {
            existing = existing[s..].to_string();
        }
    }

    let ts = super::now_epoch();
    let dt = super::chrono_date();
    let header = format!("# {} (epoch:{})\n", dt, ts);
    // Only add header if file is new or day changed
    let out = if existing.is_empty() {
        format!("{}{}\n", header, entry)
    } else {
        // Prepend header if not already present for today
        if !existing.contains(&dt) {
            format!("{}\n{}{}", header, existing, entry)
        } else {
            format!("{}{}", existing, entry)
        }
    };
    let _ = std::fs::write(&path, &out);
}

/// Append a line to a memory file, respecting max file size (~8K tokens).
pub fn append_memory(seed: &str, tier: &str, line: &str) {
    let Some(path) = super::memory_path(seed, tier) else { return };
    let _ = std::fs::create_dir_all(path.parent().unwrap());

    let mut existing = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    };

    // Keep file under ~8K tokens
    const MAX_CHARS: usize = 32000;
    if existing.len() > MAX_CHARS {
        let cut = existing.len().saturating_sub(MAX_CHARS / 2);
        existing = existing[cut..].to_string();
        // Skip to next newline for clean truncation
        if let Some(nl) = existing.find('\n') {
            existing = existing[nl + 1..].to_string();
        }
    }

    existing.push_str(line);
    existing.push('\n');
    let _ = std::fs::write(&path, &existing);
}


