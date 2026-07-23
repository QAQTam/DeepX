//! InputEngine: user input handler.
//!
//! Receives raw user text, handles auto-session-creation, compliance guard,
//! and routes to TurnEngine for LLM processing.

use deepx_proto::Agent2Ui;

use super::types::*;

pub struct InputEngine;

impl InputEngine {
    pub fn new() -> Self {
        Self
    }

    /// Handle user input. Returns an Outcome telling the Loop whether
    /// to start a turn, yield, or report an error.
    pub fn handle_user_input(&self, ctx: &mut RingContext, text: &str) -> Outcome {
        // Auto-create session on first input
        if ctx.agent.session.seed.is_empty() {
            log::info!("[INPUT] auto-creating session on first user input");
            crate::state::lifecycle::create_session(ctx.agent);
            ctx.agent.rebind_store();
            ctx.emitter.emit(Agent2Ui::SessionCreated {
                seed: ctx.agent.session.seed.clone(),
            });
            // emit_dashboard handled by MiscEngine
        }

        let text = if text == "[DeepX Goal: resume]" {
            match deepx_tools::plan::resume_goal_prompt() {
                Ok(Some(prompt)) => prompt,
                Ok(None) => "目标模式无法恢复：当前没有已暂停的步骤。".to_string(),
                Err(error) => format!("目标模式恢复失败：{error}"),
            }
        } else {
            if !text.starts_with("[自动执行计划 /") {
                deepx_tools::plan::pause_goal_for_interruption();
            }
            text.to_string()
        };

        ctx.cancel.clear();
        deepx_tools::CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);

        deepx_tools::runtime::set_context(
            &ctx.agent.session.seed,
            ctx.agent.config.permission_level,
        );

        // ── Compliance guard ──
        if ctx.agent.config.compliance_enabled {
            if let Err(reason) = deepx_gate::guard::content_guard(&text) {
                log::info!("[INPUT] compliance blocked: {reason}");
                ctx.emitter.emit(Agent2Ui::Error {
                    message: reason.clone(),
                });
                ctx.emitter.emit(Agent2Ui::TurnEnd {
                    turn_id: "blocked".into(),
                    stop_reason: Some("compliance_block".into()),
                    usage: None,
                });
                ctx.emitter.emit(Agent2Ui::Done);
                return Outcome::Handled;
            }
        }

        ctx.agent.activate_explicit_skills(&text);

        // Emit updated skills status so the frontend panel can refresh
        {
            let workspace = deepx_tools::CURRENT_WORKSPACE
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let status = ctx.agent.build_skills_status(&workspace);
            ctx.emitter.emit(Agent2Ui::SkillsChanged { status });
        }

        // Push user message
        ctx.agent.msg.push_user(&text);
        ctx.agent
            .msg
            .flush_meta(&ctx.agent.config.model, &ctx.agent.config.reasoning_effort);

        let turn_id = format!("t{}", ctx.agent.msg.turn_count());
        ctx.emitter.emit(Agent2Ui::TurnStart {
            turn_id: turn_id.clone(),
            user_text: text,
        });

        // Enter the ring: start a new turn
        Outcome::ContinueTurn {
            turn_id,
            round_num: 0,
            usage: None,
        }
    }
}
