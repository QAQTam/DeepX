//! Shared helpers for file edit tools.

/// Normalize CRLF → LF in content. Returns (normalized, was_crlf).
pub(super) fn normalize_newlines(content: &str) -> (String, bool) {
    if content.contains("\r\n") {
        (content.replace("\r\n", "\n"), true)
    } else if content.contains('\r') {
        (content.replace('\r', "\n"), true)
    } else {
        (content.to_string(), false)
    }
}

/// Find the closest line in content to the given search string.
/// Returns (line_number, line_content).
pub(super) fn closest_line(content: &str, search: &str) -> Option<(usize, String)> {
    let needle = search.lines().next().unwrap_or(search).trim();
    if needle.is_empty() { return None; }
    content.lines()
        .enumerate()
        .map(|(i, l)| (i, l, l.trim().len() as i64 - needle.len() as i64))
        .filter(|(_, l, _)| l.contains(needle) || needle.contains(l.trim()))
        .min_by_key(|(_, _, diff)| diff.unsigned_abs())
        .map(|(i, l, _)| (i + 1, l.to_string()))
}

/// Produce a unified diff between two file contents.
/// Shows the first diff region with context.
pub(crate) fn unified_diff(before: &str, after: &str, path: &str) -> String {
    let lines_a: Vec<&str> = before.lines().collect();
    let lines_b: Vec<&str> = after.lines().collect();

    if before == after {
        return String::new();
    }

    // Find first differing line
    let mut first_diff = 0usize;
    while first_diff < lines_a.len() && first_diff < lines_b.len()
        && lines_a[first_diff] == lines_b[first_diff]
    {
        first_diff += 1;
    }

    // Find last differing line by scanning from the end
    let mut end_a = lines_a.len();
    let mut end_b = lines_b.len();
    while end_a > first_diff && end_b > first_diff
        && lines_a[end_a - 1] == lines_b[end_b - 1]
    {
        end_a -= 1;
        end_b -= 1;
    }

    let ctx_start = first_diff.saturating_sub(3);
    let ctx_end = (end_a.max(end_b) + 3).min(lines_a.len().max(lines_b.len()));
    let old_count = end_a - first_diff;
    let new_count = end_b - first_diff;

    let mut diff = String::new();
    diff.push_str(&format!("--- a/{}\n+++ b/{}\n", path, path));
    diff.push_str(&format!("@@ -{},{} +{},{} @@\n",
        first_diff + 1, old_count.max(1),
        first_diff + 1, new_count.max(1)));

    // Context before
    for i in ctx_start..first_diff {
        diff.push_str(&format!(" {}\n", lines_a[i]));
    }
    // Removed lines (only the actual changed region)
    for i in first_diff..end_a {
        diff.push_str(&format!("-{}\n", lines_a[i]));
    }
    // Added lines
    for i in first_diff..end_b {
        diff.push_str(&format!("+{}\n", lines_b[i]));
    }
    // Context after
    for i in end_a.max(end_b)..ctx_end {
        let line = if i < lines_b.len() { lines_b[i] } else { lines_a[i] };
        diff.push_str(&format!(" {}\n", line));
    }

    diff
}

pub(super) fn build_diff(before: &str, after: &str, old: &str, new: &str, path: &str) -> String {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    let old_first_line = old.lines().next().unwrap_or(old);
    let change_line = before_lines.iter()
        .position(|l| l.contains(old_first_line))
        .unwrap_or(0);

    let old_count = old.lines().count();
    let new_count = new.lines().count();
    let ctx_start = change_line.saturating_sub(3);
    let ctx_end_after  = (change_line + new_count + 3).min(after_lines.len());

    let mut diff = String::new();
    // Unified diff header
    diff.push_str(&format!("--- a/{}\n+++ b/{}\n", path, path));
    // Hunk header
    let ctx_line = before_lines.get(change_line.saturating_sub(1)).unwrap_or(&"");
    diff.push_str(&format!("@@ -{},{} +{},{} @@ {}\n",
        change_line + 1, old_count.max(1),
        change_line + 1, new_count.max(1),
        ctx_line));

    // Context before
    for i in ctx_start..change_line {
        diff.push_str(&format!(" {}\n", before_lines[i]));
    }
    // Removed lines
    for i in change_line..(change_line + old_count).min(before_lines.len()) {
        diff.push_str(&format!("-{}\n", before_lines[i]));
    }
    // Added lines
    for i in change_line..(change_line + new_count).min(after_lines.len()) {
        diff.push_str(&format!("+{}\n", after_lines[i]));
    }
    // Context after
    let ctx_after_start = change_line + new_count;
    for i in ctx_after_start..ctx_end_after {
        diff.push_str(&format!(" {}\n", after_lines[i]));
    }

    diff
}

/// Score candidates by context-before/context-after proximity and pick the best match.
/// Returns Ok(index) on success, or Err(partial-message) when context is missing.
pub(super) fn disambiguate_match(
    candidates: &[usize],
    context_before: &[String],
    context_after: &[String],
    file_lines: &[&str],
    path: &str,
    win: usize,
) -> Result<usize, String> {
    if candidates.len() == 1 {
        return Ok(candidates[0]);
    }
    let norm_before: Vec<String> = context_before.iter().map(|l| l.trim_end().to_string()).collect();
    let norm_after: Vec<String> = context_after.iter().map(|l| l.trim_end().to_string()).collect();
    if norm_before.is_empty() && norm_after.is_empty() {
        let locs: Vec<String> = candidates.iter().take(5).map(|&i| format!("L{}", i+1)).collect();
        return Err(format!("[PARTIAL] {} — old_lines matches at {} locations: {}\n[HINT] Add context_before/context_after to disambiguate.", path, candidates.len(), locs.join(", ")));
    }
    let mut best = candidates[0];
    let mut best_score: i32 = -1000;
    for &pos in candidates {
        let mut score = 0i32;
        for (j, cl) in norm_before.iter().enumerate() {
            let fi = pos as i32 - norm_before.len() as i32 + j as i32;
            if fi >= 0 && (fi as usize) < file_lines.len() {
                let fl = file_lines[fi as usize].trim_end().to_string();
                if fl == *cl { score += 3; } else if fl.trim() == cl.trim() { score += 1; } else { score -= 1; }
            } else { score -= 2; }
        }
        for (j, cl) in norm_after.iter().enumerate() {
            let fi = pos + win + j;
            if fi < file_lines.len() {
                let fl = file_lines[fi].trim_end().to_string();
                if fl == *cl { score += 3; } else if fl.trim() == cl.trim() { score += 1; } else { score -= 1; }
            } else { score -= 2; }
        }
        if score > best_score { best = pos; best_score = score; }
    }
    Ok(best)
}

/// Apply the diff (remove old_lines, insert new_lines) and format the result.
pub(super) fn apply_diff_and_format(
    path: &str,
    file_lines: &[&str],
    match_idx: usize,
    win: usize,
    new_lines: &[String],
    description: &str,
    was_fuzzy: bool,
) -> String {
    let mut out_lines: Vec<&str> = file_lines.to_vec();
    out_lines.splice(match_idx..match_idx + win, std::iter::empty());
    for (j, line) in new_lines.iter().enumerate() {
        out_lines.insert(match_idx + j, line);
    }
    let new_content = out_lines.join("\n");

    match std::fs::write(path, &new_content) {
        Ok(_) => {
            let line = match_idx + 1;
            let added = new_lines.len() as u32;
            let removed = win as u32;
            let mut result = String::new();
            if was_fuzzy {
                result.push_str("\u{26a0} fuzzy match (indentation normalized)\n");
            }
            // Unified diff header
            result.push_str(&format!("--- a/{}\n+++ b/{}\n", path, path));
            // Hunk header
            let ctx_line = file_lines.get(match_idx.saturating_sub(1)).unwrap_or(&"");
            result.push_str(&format!("@@ -{},{} +{},{} @@ {}\n",
                line, removed.max(1), line, added.max(1), ctx_line));
            // Context before
            let ctx_start = match_idx.saturating_sub(2);
            for i in ctx_start..match_idx {
                result.push_str(&format!(" {}\n", file_lines[i]));
            }
            // Removed lines
            for i in match_idx..match_idx + win {
                result.push_str(&format!("-{}\n", file_lines[i]));
            }
            // Added lines
            for l in new_lines {
                result.push_str(&format!("+{}\n", l));
            }
            // Context after
            let ctx_end = (match_idx + win + 2).min(out_lines.len());
            for i in (match_idx + win)..ctx_end {
                result.push_str(&format!(" {}\n", out_lines[i]));
            }
            let desc = if description.is_empty() { "edited" } else { description };
            result.push_str(&format!("\n[CHANGE] {}:{} +{} -{} | {}", path, line, added, removed, desc));
            result
        }
        Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify parent directory exists and is writable.", path, e),
    }
}
