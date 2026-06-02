//! Shared helpers for file edit tools.

pub(super) fn build_diff(before: &str, after: &str, old: &str, new: &str, path: &str) -> String {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    let change_line = before_lines.iter()
        .position(|l| l.contains(old.lines().next().unwrap_or(old)))
        .unwrap_or(0);

    let ctx_start = change_line.saturating_sub(3);
    let _ctx_end = (change_line + 3).min(before_lines.len());

    let mut diff = String::new();
    diff.push_str(&format!("  {}  (line {})\n", path, change_line + 1));

    for i in ctx_start..change_line {
        diff.push_str(&format!("      {:>4}  {}\n", i + 1, before_lines[i]));
    }

    let before_snippet: Vec<&str> = before_lines[change_line..(change_line + old.lines().count()).min(before_lines.len())].to_vec();
    let after_snippet: Vec<&str> = after_lines[change_line..(change_line + new.lines().count()).min(after_lines.len())].to_vec();

    for bl in &before_snippet {
        diff.push_str(&format!("  -   {:>4}  {}\n", change_line + 1, bl));
    }
    for al in &after_snippet {
        diff.push_str(&format!("  +   {:>4}  {}\n", change_line + 1, al));
    }

    let after_change = change_line + new.lines().count();
    for i in after_change..(after_change + 3).min(after_lines.len()) {
        diff.push_str(&format!("      {:>4}  {}\n", i + 1, after_lines[i]));
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
    let added = new_lines.len() as u32;
    let removed = win as u32;

    match std::fs::write(path, &new_content) {
        Ok(_) => {
            let line = match_idx + 1;
            let mut result = format!("[OK] {}:{}\n", path, line);
            if was_fuzzy {
                result.push_str("\u{26a0} fuzzy match (indentation normalized)\n");
            }
            let ctx_start = match_idx.saturating_sub(2);
            let ctx_end = (match_idx + win + 2).min(out_lines.len()).max(match_idx + 1);
            result.push_str("\u{2500}\u{2500} change \u{2500}\u{2500}\n");
            for i in ctx_start..match_idx {
                result.push_str(&format!("  {:>4}  {}\n", i+1, file_lines[i]));
            }
            for i in match_idx..match_idx + win {
                result.push_str(&format!("- {:>4}  {}\n", i+1, file_lines[i]));
            }
            for (j, l) in new_lines.iter().enumerate() {
                result.push_str(&format!("+ {:>4}  {}\n", match_idx + 1 + j, l));
            }
            for i in (match_idx + win)..ctx_end {
                if i < out_lines.len() {
                    result.push_str(&format!("  {:>4}  {}\n", i+1, out_lines[i]));
                }
            }
            let desc = if description.is_empty() { "edited" } else { description };
            result.push_str(&format!("\n[CHANGE] {}:{} +{} -{} | {}", path, line, added, removed, desc));
            result
        }
        Err(e) => format!("[ERROR] Cannot write {}: {}\n[HINT] Verify parent directory exists and is writable.", path, e),
    }
}
