// ── Type definitions for DSX core types ──
//
// All type definitions are split across sub-modules below.
// This file re-exports every public symbol so consumers can
// use `dsx_types::TypeName` without caring about sub-module layout.

// ── Sub-module declarations (each file = one logical group) ──

pub mod message;
pub mod tool_def;
pub mod state;
pub mod config;
pub mod session;
pub mod api_types;


// Unified arg parsing (shared across dsx-agent, dsx-tools)
pub mod arg;

// Platform-specific utilities
pub mod platform;

pub mod token;

// ── Re-exports: flat public API ──

pub use message::{Message, ContentBlock, ToolCall, FunctionCall};
pub use tool_def::{ToolDef, ToolFunction};
pub use state::{TaskPhase, DebugLevel};
pub use config::{PersistentConfig, PhasePerfConfig, ProfileConfig, default_phase_configs};
pub use session::{SessionFile, SessionMeta};
pub use api_types::UsageInfo;

// ── Unified arg parsers ──
pub use arg::{
    parse_arg, parse_arg_or, parse_opt, parse_opt_u64, tool_action, parse_file_arg, parse_cmd_arg,
};

// ── Shared utilities ──
pub use token::{TokenBreakdown, count_tokens};
