//! Safety verdicts for tool invocation.
//!
//! Centralized safety classification using [`ToolRisk`] levels
//! evaluated against whether the operation is inside the workspace.

use crate::ToolRisk;

/// The result of a tool safety check.
#[derive(Debug, Clone)]
pub enum SafetyVerdict {
    /// Automatically allow execution.
    Allow,
    /// Require authentication before execution.
    RequireAuth { reason: String },
    /// Block execution with the given reason.
    Block(String),
}

/// Static safety policy — evaluates a [`ToolRisk`] / in-workspace pair
/// into a [`SafetyVerdict`].
pub struct SafetyPolicy;

impl SafetyPolicy {
    /// Evaluate whether a tool call with the given risk level, inside or
    /// outside the workspace, should be allowed, require auth, or blocked.
    pub fn evaluate(risk: ToolRisk, in_workspace: bool) -> SafetyVerdict {
        match (risk, in_workspace) {
            (ToolRisk::ReadOnly, _) => SafetyVerdict::Allow,
            (ToolRisk::Write, true) => SafetyVerdict::Allow,
            (ToolRisk::Write, false) => SafetyVerdict::RequireAuth {
                reason: "Write operation outside workspace requires confirmation".into(),
            },
            (ToolRisk::Destructive, true) => SafetyVerdict::RequireAuth {
                reason: "Destructive operation requires confirmation".into(),
            },
            (ToolRisk::Destructive, false) => SafetyVerdict::Block(
                "Destructive operation outside workspace is blocked".into(),
            ),
            (ToolRisk::Administrative, _) => SafetyVerdict::RequireAuth {
                reason: "Administrative operation requires confirmation".into(),
            },
        }
    }
}
