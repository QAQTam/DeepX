use super::AgentState;
use crate::orchestrator::tracker;
use crate::tools::wrap_tool_result;

#[derive(Debug, Clone)]
pub struct FileSnapshot {
    pub lines: usize,
    pub hash: u64,
    pub last_read_turn: u32,
}

impl FileSnapshot {
    pub(crate) fn hash_of(path: &str) -> u64 {
        use std::hash::Hasher;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if let Ok(meta) = std::fs::metadata(path) {
            std::hash::Hash::hash(&meta.len(), &mut h);
            if let Ok(m) = meta.modified() {
                std::hash::Hash::hash(&m, &mut h);
            }
        }
        h.finish()
    }
}

pub struct ToolResultAppender<'a> {
    pub state: &'a mut AgentState,
}

impl<'a> ToolResultAppender<'a> {
    pub fn new(state: &'a mut AgentState) -> Self {
        Self { state }
    }

    /// Append a tool result to the context and record all side effects.
    pub fn append(&mut self, tool_name: &str, tc_id: &str, args: &str, raw: &str) -> bool {
        // Global size gate: any tool result > 50K chars gets truncated
        // to prevent LLM context bloat regardless of per-tool limits.
        const MAX_TOOL_RESULT_CHARS: usize = 50_000;
        let truncated = if raw.len() > MAX_TOOL_RESULT_CHARS {
            let mut t = raw[..MAX_TOOL_RESULT_CHARS].to_string();
            t.push_str(&format!("\n...[TRUNCATED: {} total chars, showing first {MAX_TOOL_RESULT_CHARS}]", raw.len()));
            t
        } else {
            raw.to_string()
        };

        let failed = raw.starts_with("[ERROR]") || raw.starts_with("[FAIL]");
        let result = wrap_tool_result(tool_name, &truncated);

        if let Err(e) = self.state.ctx.push_tool_result(tc_id, &result) {
            log::warn!("ToolResultAppender: push_tool_result failed for {}: {:?}", tc_id, e);
            let _ = self.state.ctx.push_tool_result_for(tc_id, &result);
        }

        self.state.tool_results.push((tool_name.to_string(), result.clone()));

        if !failed && (tool_name == "write_file" || tool_name == "edit_file") {
            tracker::track_file_written(self.state, args);
            if let Some(path) = dsx_types::arg::parse_file_arg(args) {
                self.state.re_read_required = Some(path.clone());
                // Push diff context with timestamp: model sees edit history
                let label = format!("file:{}", path);
                let ctx_lines: Vec<&str> = raw.lines().filter(|l| l.starts_with("  ") || l.starts_with("+") || l.starts_with("-")).collect();
                if !ctx_lines.is_empty() {
                    let ts = chrono::Local::now().format("%H:%M:%S");
                    let entry: Vec<String> = ctx_lines.iter().map(|l| format!("[{}] {}", ts, l)).collect();
                    self.state.append_context(&label, &entry.join("\n"));
                }

                // Append edit history log for the model to see its own changes
                let ts = chrono::Local::now().format("%H:%M:%S");
                let tag = crate::orchestrator::learning::doc_tag(&path);
                let summary = extract_edit_summary(raw, &path);

                let reason = serde_json::from_str::<serde_json::Value>(args)
                    .ok()
                    .and_then(|v| v.get("reason")
                        .and_then(|r| r.as_str())
                        .filter(|s| !s.is_empty())
                        .map(String::from));

                let line = if let Some(r) = reason {
                    format!("[{}] tag:{} {} — {}  ← {}\n", ts, tag, tool_name, summary, r)
                } else {
                    format!("[{}] tag:{} {} — {}\n", ts, tag, tool_name, summary)
                };
                self.state.append_context("edit:log", &line);
            }
        }

        // Push explore results to context for stable KV cache prefix
        if !failed && tool_name == "explore" {
            let lines: Vec<&str> = raw.lines().filter(|l| !l.is_empty()).take(80).collect();
            if !lines.is_empty() {
                self.state.push_context("project:map", &lines.join("\n"));
            }
        }

        !failed
    }
}

/// Extract a concise edit summary from raw tool output.
fn extract_edit_summary(raw: &str, path: &str) -> String {
    let first_line = raw.lines().next().unwrap_or("");
    if raw.contains("appended") {
        format!("{} (append)", path)
    } else if let Some(line_info) = first_line.strip_prefix("[OK] ") {
        // e.g. "[OK] src/main.rs:42 +10 -5" → "src/main.rs: lines 42 (+10/-5)"
        let short = line_info.replace(path, "").trim().trim_start_matches(':').to_string();
        format!("{} ({})", path, short)
    } else {
        format!("{}", path)
    }
}
