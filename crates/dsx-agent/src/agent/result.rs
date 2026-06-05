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
            let end = raw.floor_char_boundary(MAX_TOOL_RESULT_CHARS);
            let mut t = raw[..end].to_string();
            let total_chars = raw.chars().count();
            t.push_str(&format!("\n...[TRUNCATED: {total_chars} total chars, showing first {MAX_TOOL_RESULT_CHARS}]"));
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
            }
        }

        !failed
    }
}
