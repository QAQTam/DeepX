//! CompactEngine: context compaction — token-split → prompt → LLM → apply.
//!
//! Two-step flow:
//! 1. `build_prompt_and_meta()` — synchronous, fast (token split + prompt build)
//! 2. Background: `chat_stream()` call in a thread (non-blocking, streaming
//!    tokens to frontend via CompactDelta events)
//! 3. `apply_result()` — synchronous, fast (apply on main thread)

use deepx_proto::Agent2Ui;
use deepx_session::SessionManager;

use super::types::*;
use crate::util;

/// Result produced by the background compact thread.
pub(crate) struct CompactMeta {
    pub summary: String,
    pub kept_user_count: usize,
    pub head_user_count: usize,
    pub error: Option<String>,
}

/// Compaction prompt: instructs the LLM to produce a handoff summary
/// for another LLM instance that will resume the task.
const COMPACT_PROMPT: &str = "\
You are performing a CONTEXT CHECKPOINT COMPACTION. Create a handoff \
summary for another LLM that will resume the task.\n\
\n\
Include:\n\
- Current progress and key decisions made\n\
- Important context, constraints, or user preferences\n\
- What remains to be done (clear next steps)\n\
- Any critical data, examples, or references needed to continue\n\
- File paths that were created, modified, or deleted\n\
- Errors encountered and how they were resolved (or not)\n\
\n\
Be concise, structured, and focused on helping the next LLM seamlessly \
continue the work. Do not mention the compaction process itself.";

/// Prefix injected before a previous summary in UPDATE MODE,
/// telling the LLM that this summary came from a prior instance.
const SUMMARY_PREFIX: &str = "\
Another language model started to solve this problem and produced a \
summary of its thinking process. You also have access to the state of \
the tools that were used by that language model. Use this to build on \
the work that has already been done and avoid duplicating work. Here \
is the summary produced by the other language model, use the \
information in this summary to assist with your own analysis:";

pub struct CompactEngine;

impl CompactEngine {
    pub fn new() -> Self {
        Self
    }
    pub fn reset(&mut self) {}

    /// Step 1: Token-split, serialize, build prompt — fast, synchronous.
    /// Returns (prompt, kept_user_count, head_user_count, provider) needed
    /// for the LLM call and apply step. Returns None if no compaction needed.
    pub(crate) fn build_prompt_and_meta(
        &self,
        ctx: &mut RingContext,
    ) -> Option<(String, usize, usize, deepx_gate::ProviderConfig)> {
        const KEEP_TOKENS: usize = 4_000;
        let turns_total = ctx.agent.msg.turn_count();
        log::info!("[COMPACT] {} turns", turns_total);

        let all = ctx
            .agent
            .msg
            .build_context_for_gate(&[]);
        let msgs: Vec<&deepx_types::Message> = all.iter().filter(|m| m.role != "system").collect();
        if msgs.is_empty() {
            return None;
        }

        let estimate = |s: &str| -> usize { s.chars().count() / 4 };
        let mut kept_idx = msgs.len();
        let mut kept_tokens = 0usize;
        for (i, m) in msgs.iter().enumerate().rev() {
            let t = estimate(&serde_json::to_string(m).unwrap_or_default());
            if kept_tokens + t > KEEP_TOKENS {
                kept_idx = i + 1;
                break;
            }
            kept_tokens += t;
            kept_idx = i;
        }
        let head_msgs = &msgs[..kept_idx];
        if head_msgs.is_empty() {
            ctx.emitter.emit_delta(Agent2Ui::ToolNotice {
                message: "Compact skipped: all within token budget".into(),
                level: "info".into(),
            });
            return None;
        }

        let head_user_count = head_msgs.iter().filter(|m| m.role == "user").count();
        let kept_user_count = msgs[kept_idx..].iter().filter(|m| m.role == "user").count();

        ctx.emitter.emit(Agent2Ui::CompactStart {
            turns_total: turns_total as u32,
            turns_keeping: kept_user_count as u32,
        });

        let contexts = serialize_messages(head_msgs, &msgs[kept_idx..]);
        let timeline = {
            let created = ctx.agent.session.created_at;
            let updated = ctx
                .agent
                .session
                .updated_at
                .max(SessionManager::now_epoch());
            let start_str = util::epoch_to_date(created);
            let dur = updated.saturating_sub(created);
            format!(
                "- Session started: {start_str} (UTC)\n- Session duration: {}h {}m real-time",
                dur / 3600,
                (dur % 3600) / 60
            )
        };

        let previous_summary = ctx.agent.msg.previous_compact_summary();
        let prompt = if let Some(ref prev) = previous_summary {
            format!(
                "[COMPACT — UPDATE MODE]\n\n\
                 {SUMMARY_PREFIX}\n\n\
                 <previous-summary>\n{prev}\n</previous-summary>\n\n\
                 --- HISTORY (newer context to merge) ---\n\
                 {}\n\
                 --- END HISTORY ---\n\n\
                 {COMPACT_PROMPT}",
                contexts.join("\n\n"),
            )
        } else {
            format!(
                "[COMPACT]\n\n\
                 Create a new checklist summary from the conversation history.\n\n\
                 --- HISTORY ---\n\
                 {}\n\
                 --- END HISTORY ---\n\n\
                 Timeline:\n{timeline}\n\n\
                 {COMPACT_PROMPT}",
                contexts.join("\n\n"),
            )
        };

        let provider = deepx_gate::ProviderConfig::openai(
            &ctx.agent.config.base_url,
            &ctx.agent.config.api_key,
            &ctx.agent.config.model,
            None,
            None,
            Default::default(),
            Default::default(),
            false,
            None,
        );
        Some((prompt, kept_user_count, head_user_count, provider))
    }

    /// Step 2: Apply compact result on the live message store (called from main thread).
    pub(crate) fn apply_result(&self, ctx: &mut RingContext, meta: &CompactMeta) {
        if let Some(ref err) = meta.error {
            ctx.emitter.emit(Agent2Ui::Error {
                message: err.clone(),
            });
            ctx.emitter.emit(Agent2Ui::CompactEnd {
                summary_chars: 0,
                turns_compacted: 0,
            });
            return;
        }
        let chars = meta.summary.chars().count();
        ctx.agent
            .msg
            .apply_compact(&meta.summary, meta.kept_user_count);
        ctx.agent
            .msg
            .snapshot_full(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);

        let (
            chat_text,
            thinking,
            tool_calls,
            tool_results,
            tools_schema,
            system_prompt,
            thinking_blocks,
            tool_call_blocks,
        ) = ctx
            .agent
            .msg
            .compute_context_stats(Some(&ctx.agent.tool_defs));
        let stats = serde_json::json!({
            "messages": ctx.agent.msg.turn_count(),
            "chat_text": chat_text, "thinking": thinking,
            "tool_calls": tool_calls, "tool_results": tool_results,
            "tools_schema": tools_schema, "system_prompt": system_prompt,
            "thinking_blocks": thinking_blocks, "tool_call_blocks": tool_call_blocks,
        });
        let stats_dir = deepx_types::platform::sessions_dir().join(&ctx.agent.session.seed);
        let _ = std::fs::create_dir_all(&stats_dir);
        let _ = std::fs::write(stats_dir.join("context_stats.json"), stats.to_string());

        ctx.emitter.emit(Agent2Ui::CompactEnd {
            summary_chars: chars,
            turns_compacted: meta.head_user_count as u32,
        });
        ctx.emitter.emit_delta(Agent2Ui::ToolNotice {
            message: format!(
                "Compacted {} turns -> {chars} chars, keeping {} turns",
                meta.head_user_count, meta.kept_user_count,
            ),
            level: "info".into(),
        });
    }
}

// ═══════════════════════════════════════════════════════
// Background worker — runs in a separate thread
// ═══════════════════════════════════════════════════════

/// Run the LLM compaction call in a background thread.
/// Uses streaming so the user can see the model output in real-time
/// via `CompactDelta` events pushed through `event_tx`.
/// Returns CompactMeta via the channel.
pub(crate) fn run_compact_worker(
    prompt: String,
    provider: deepx_gate::ProviderConfig,
    kept_user_count: usize,
    head_user_count: usize,
    event_tx: std::sync::mpsc::SyncSender<deepx_proto::Agent2Ui>,
) -> CompactMeta {
    let msgs_vec = vec![deepx_types::Message::user(&prompt)];
    let mut summary = String::new();

    let mut on_event = |ev: deepx_gate::StreamEvent| match ev {
        deepx_gate::StreamEvent::ContentDelta(delta) => {
            summary.push_str(&delta);
            let _ = event_tx.send(deepx_proto::Agent2Ui::CompactDelta { delta });
        }
        deepx_gate::StreamEvent::ReasoningDelta(delta) => {
            summary.push_str(&delta);
            let _ = event_tx.send(deepx_proto::Agent2Ui::CompactDelta { delta });
        }
        _ => {}
    };

    match deepx_gate::chat_stream(
        &provider,
        msgs_vec,
        None,
        20480,
        None,
        None,
        None,
        &mut on_event,
    ) {
        Ok(()) if !summary.trim().is_empty() => CompactMeta {
            summary,
            kept_user_count,
            head_user_count,
            error: None,
        },
        Ok(()) => CompactMeta {
            summary: String::new(),
            kept_user_count,
            head_user_count,
            error: Some("Compact failed: model returned empty response.".into()),
        },
        Err(e) => CompactMeta {
            summary: String::new(),
            kept_user_count,
            head_user_count,
            error: Some(format!("{e}")),
        },
    }
}

// ═══════════════════════════════════════════════════════
// Message serialization helpers
// ═══════════════════════════════════════════════════════

fn serialize_messages(
    head: &[&deepx_types::Message],
    kept: &[&deepx_types::Message],
) -> Vec<String> {
    let mut out = Vec::new();
    for m in head {
        let role = &m.role;
        let lines: Vec<String> = m
            .content
            .iter()
            .filter_map(|b| match b {
                deepx_types::ContentBlock::Text { text } => Some(format!("[{role}]: {text}")),
                deepx_types::ContentBlock::Reasoning { .. } => None,
                deepx_types::ContentBlock::ToolUse { name, input, .. } => {
                    let args = serde_json::to_string(input).unwrap_or_default();
                    let end = args.floor_char_boundary(args.len().min(120));
                    Some(format!("[{role} tool call]: {}({})", name, &args[..end]))
                }
                deepx_types::ContentBlock::ToolResult { content, .. } => {
                    let compact: String = content
                        .lines()
                        .take(5)
                        .map(|l| l.chars().take(200).collect::<String>())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    let end = compact.floor_char_boundary(compact.len().min(600));
                    Some(format!("[Tool result]: {}", &compact[..end]))
                }
            })
            .collect();
        if !lines.is_empty() {
            out.push(lines.join("\n"));
        }
    }
    for m in kept {
        if m.role == "tool" {
            if let Some(deepx_types::ContentBlock::ToolResult { content, .. }) = m.content.first() {
                let compact: String = content
                    .lines()
                    .take(3)
                    .map(|l| l.chars().take(200).collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" | ");
                let end = compact.floor_char_boundary(compact.len().min(400));
                out.push(format!("[Tool result (recent)]: {}", &compact[..end]));
            }
        }
    }
    out
}
