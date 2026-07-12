//! Context compaction: summarize old conversation turns via LLM.
//!
//! Uses token-driven split to keep recent context intact and summarizes
//! older turns into a compact anchored summary. Supports incremental
//! updates (update mode) when previous summaries exist.

use crate::Loop;
use crate::util;
use deepx_proto::Agent2Ui;
use deepx_session::SessionManager;

/// Structured template for compaction summary output.
/// Forces the LLM to produce sections that preserve file paths, errors, and next actions.
const COMPACT_TEMPLATE: &str = "\
Output exactly the Markdown structure shown inside <template> and keep the section order unchanged. \
Do not include the <template> tags in your response.\n\
<template>\n\
## Objective\n\
- [one or two brief sentences describing what the user is trying to accomplish]\n\n\
## Important Details\n\
- [constraints/preferences, decisions and why, important facts/assumptions, \
exact context needed to continue, or \"(none)\"]\n\n\
## File Inventory\n\
- Added: [new files with paths, or \"(none)\"]\n\
- Modified: [changed files with paths and what changed, or \"(none)\"]\n\
- Deleted: [removed files with paths, or \"(none)\"]\n\n\
## Decision Log\n\
- [key trade-offs made: why approach A over B, rejected alternatives and rationale; otherwise \"(none)\"]\n\n\
## Key Symbols\n\
- [function signatures, type names, trait impls, API routes, config keys that are essential to resume work; otherwise \"(none)\"]\n\n\
## Work State\n\
- Completed: [finished work, verified facts, or FILES created/modified/deleted with paths; otherwise \"(none)\"]\n\
- Active: [current work, partial changes, or investigation state; otherwise \"(none)\"]\n\
- Blocked: [blockers, errors encountered and resolutions, or unknowns; otherwise \"(none)\"]\n\n\
## Next Move\n\
1. [immediate concrete action, or \"(none)\"]\n\
2. [next action if known, or \"(none)\"]\n\
</template>\n\n\
Rules:\n\
- Keep every section, even when empty.\n\
- Use terse bullets, not prose paragraphs.\n\
- Preserve exact file paths, symbols, commands, error strings, URLs, and identifiers when known.\n\
- Put relevant files and symbols inside the section where they matter; do not add extra sections.\n\
- Do not mention the summary process or that context was compacted.";

pub(crate) fn handle_compact(loop_ref: &mut Loop) {
    const KEEP_TOKENS: usize = 4_000; // token budget for recent context to keep intact
    let turns_total = loop_ref.agent.msg.turn_count();
    log::info!("[AGENT] handle_compact: {} turns", turns_total);

    // Build full message list (excluding system messages) for token-driven split.
    let all = loop_ref.agent.msg.build_context_for_gate(&[]);
    let msgs: Vec<&deepx_types::Message> = all.iter().filter(|m| m.role != "system").collect();
    if msgs.is_empty() {
        return;
    }

    // Token-driven split: scan from end, accumulate estimated tokens
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
    let head_msgs = &msgs[..kept_idx]; // messages to summarize
    if head_msgs.is_empty() {
        loop_ref.emit_delta(Agent2Ui::ToolNotice {
            message: "Compact skipped: nothing to compact (all within token budget)".into(),
            level: "info".into(),
        });
        return;
    }

    // Count how many turns are in the head (being compacted)
    let head_user_count = head_msgs.iter().filter(|m| m.role == "user").count();
    // Count kept turns
    let kept_user_count = msgs[kept_idx..].iter().filter(|m| m.role == "user").count();

    loop_ref.emit(Agent2Ui::CompactStart {
        turns_total: turns_total as u32,
        turns_keeping: kept_user_count as u32,
    });

    // ── Serialize head messages into a dense text format for the compactor LLM ──
    let mut contexts = Vec::new();
    for m in head_msgs {
        let role = &m.role;
        let serialized: Vec<String> = m
            .content
            .iter()
            .filter_map(|b| match b {
                deepx_types::ContentBlock::Text { text } => {
                    Some(format!("[{}]: {}", role, text))
                }
                deepx_types::ContentBlock::Reasoning { reasoning } => Some(format!(
                    "[{} reasoning]: {}",
                    role,
                    &reasoning[..reasoning.floor_char_boundary(reasoning.len().min(500))]
                )),
                deepx_types::ContentBlock::ToolUse { name, input, .. } => {
                    let args = serde_json::to_string(input).unwrap_or_default();
                    Some(format!(
                        "[{} tool call]: {}({})",
                        role,
                        name,
                        &args[..args.floor_char_boundary(args.len().min(120))]
                    ))
                }
                deepx_types::ContentBlock::ToolResult { content, .. } => {
                    let compact: String = content
                        .lines()
                        .take(5)
                        .map(|l| l.chars().take(200).collect::<String>())
                        .collect::<Vec<_>>()
                        .join(" | ");
                    Some(format!(
                        "[Tool result]: {}",
                        &compact[..compact.floor_char_boundary(compact.len().min(600))]
                    ))
                }
            })
            .collect();
        if !serialized.is_empty() {
            contexts.push(serialized.join("\n"));
        }
    }
    // Also serialize the TOOL use/results from kept messages for context about what's happening right now
    for m in &msgs[kept_idx..] {
        let role = &m.role;
        if role == "tool" {
            if let Some(deepx_types::ContentBlock::ToolResult { content, .. }) =
                m.content.first()
            {
                let compact: String = content
                    .lines()
                    .take(3)
                    .map(|l| l.chars().take(200).collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" | ");
                contexts.push(format!(
                    "[Tool result (recent)]: {}",
                    &compact[..compact.floor_char_boundary(compact.len().min(400))]
                ));
            }
        }
    }

    // ── Timeline: session creation time + duration ──
    let timeline = {
        let created = loop_ref.agent.session.created_at;
        let updated = loop_ref
            .agent
            .session
            .updated_at
            .max(SessionManager::now_epoch());
        let start_str = util::epoch_to_date(created);
        let dur = updated.saturating_sub(created);
        let dur_hours = dur / 3600;
        let dur_min = (dur % 3600) / 60;
        format!(
            "- Session started: {} (UTC)\n- Session duration: {}h {}m real-time",
            start_str, dur_hours, dur_min
        )
    };

    // ── Incremental summary: detect previous compact for update mode ──
    let previous_summary = loop_ref.agent.msg.previous_compact_summary();

    // ── Build prompt ──
    let prompt = if let Some(ref prev) = previous_summary {
        format!(
            "[COMPACT — UPDATE MODE]\n\n\
             Update the anchored summary below using the stripped conversation history.\n\
             Preserve still-true details, remove stale details, merge in new facts.\n\n\
             <previous-summary>\n{}\n</previous-summary>\n\n\
             --- HISTORY (newer context to merge) ---\n{}\n--- END HISTORY ---\n\n\
             {}",
            prev,
            contexts.join("\n\n"),
            COMPACT_TEMPLATE,
        )
    } else {
        format!(
            "[COMPACT]\n\n\
             Create a new anchored summary from the stripped conversation history.\n\n\
             --- HISTORY ---\n{}\n--- END HISTORY ---\n\n\
             {}\n\n\
             {}",
            contexts.join("\n\n"),
            timeline,
            COMPACT_TEMPLATE,
        )
    };

    let provider = deepx_gate::ProviderConfig::openai(
        &loop_ref.agent.config.base_url,
        &loop_ref.agent.config.api_key,
        &loop_ref.agent.config.model,
        None,
        None,
        Default::default(),
        Default::default(),
        false,
    );
    let msgs_vec = vec![deepx_types::Message::user(&prompt)];
    let summary = match deepx_gate::chat_sync(&provider, msgs_vec, 4096) {
        Ok(s) if !s.trim().is_empty() => s,
        Ok(_) => {
            loop_ref.emit(Agent2Ui::Error {
                message: "Compact failed: model returned empty response. Try again.".into(),
            });
            loop_ref.emit(Agent2Ui::CompactEnd {
                summary_chars: 0,
                turns_compacted: 0,
            });
            return;
        }
        Err(e) => {
            loop_ref.emit(Agent2Ui::Error { message: e });
            loop_ref.emit(Agent2Ui::CompactEnd {
                summary_chars: 0,
                turns_compacted: 0,
            });
            return;
        }
    };

    let chars = summary.chars().count();
    let keep_turns = kept_user_count;
    loop_ref.agent.msg.apply_compact(&summary, keep_turns);
    loop_ref.agent.msg.snapshot_full(
        &loop_ref.agent.config.model,
        &loop_ref.agent.config.reasoning_effort,
    );

    // Write post-compact context stats
    {
        let (
            chat_text,
            thinking,
            tool_calls,
            tool_results,
            tools_schema,
            system_prompt,
            thinking_blocks,
            tool_call_blocks,
        ) = loop_ref
            .agent
            .msg
            .compute_context_stats(Some(&loop_ref.agent.tool_defs));
        let stats = serde_json::json!({
            "messages": loop_ref.agent.msg.turn_count(),
            "chat_text": chat_text,
            "thinking": thinking,
            "tool_calls": tool_calls,
            "tool_results": tool_results,
            "tools_schema": tools_schema,
            "system_prompt": system_prompt,
            "thinking_blocks": thinking_blocks,
            "tool_call_blocks": tool_call_blocks,
        });
        let stats_dir = deepx_types::platform::sessions_dir().join(&loop_ref.agent.session.seed);
        let _ = std::fs::create_dir_all(&stats_dir);
        let stats_path = stats_dir.join("context_stats.json");
        let _ = std::fs::write(&stats_path, stats.to_string());
    }

    loop_ref.emit(Agent2Ui::CompactEnd {
        summary_chars: chars,
        turns_compacted: head_user_count as u32,
    });
    loop_ref.emit_delta(Agent2Ui::ToolNotice {
        message: format!(
            "Compacted {} turns -> {} chars, keeping {} turns",
            head_user_count, chars, keep_turns
        ),
        level: "info".into(),
    });
    loop_ref.emit_dashboard();
}
