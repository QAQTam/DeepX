//! Write-conflict detection for tool execution.
//!
//! Detects same-file write conflicts among pending tool calls and groups them
//! into serial execution sets to avoid race conditions.

use std::collections::{HashMap, HashSet};

use serde_json;

/// Extract file paths that a tool writes to (mutates).
/// Returns empty vec for read-only and non-file tools.
pub(crate) fn file_write_paths(tool_name: &str, args: &serde_json::Value) -> Vec<String> {
    if tool_name != "file" {
        return Vec::new();
    }
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let mut paths = Vec::new();
    // All actions that modify files
    match action {
        "write" | "edit" | "edit_diff" | "delete" => {
            if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
                paths.push(p.to_string());
            }
            if let Some(arr) = args.get("paths").and_then(|v| v.as_array()) {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        paths.push(s.to_string());
                    }
                }
            }
        }
        "move" | "copy" => {
            // Both source and dest are affected; dest is the write target
            if let Some(p) = args.get("dest").and_then(|v| v.as_str()) {
                paths.push(p.to_string());
            }
            if let Some(p) = args.get("source").and_then(|v| v.as_str()) {
                paths.push(p.to_string());
            }
        }
        _ => {}
    }
    paths
}

/// Detect same-file write conflicts among pending tools and group them
/// into serial execution sets. Returns (serial_groups, serial_after_indices).
pub(crate) fn resolve_write_conflicts(
    pending: &[deepx_message::PendingTool],
) -> (Vec<Vec<usize>>, HashSet<usize>) {
    let mut file_writers: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, tool) in pending.iter().enumerate() {
        for path in file_write_paths(&tool.name, &tool.args) {
            file_writers.entry(path).or_default().push(i);
        }
    }
    let mut serial_groups: Vec<Vec<usize>> = Vec::new();
    {
        let mut visited = vec![false; pending.len()];
        for indices in file_writers.values() {
            if indices.is_empty() {
                continue;
            }
            let rep = indices[0];
            if visited[rep] {
                continue;
            }
            let mut group_set: HashSet<usize> = HashSet::new();
            let mut stack: Vec<usize> = indices.clone();
            while let Some(idx) = stack.pop() {
                if !group_set.insert(idx) {
                    continue;
                }
                visited[idx] = true;
                for other in file_writers.values() {
                    if other.contains(&idx) {
                        for &oi in other {
                            if !group_set.contains(&oi) {
                                stack.push(oi);
                            }
                        }
                    }
                }
            }
            let mut group: Vec<usize> = group_set.into_iter().collect();
            group.sort();
            if group.len() > 1 {
                serial_groups.push(group);
            }
        }
    }
    let mut serial_after: HashSet<usize> = HashSet::new();
    for group in &serial_groups {
        for &idx in &group[1..] {
            serial_after.insert(idx);
        }
    }
    (serial_groups, serial_after)
}
