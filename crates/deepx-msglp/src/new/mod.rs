//! new/ — experimental new Loop architecture (Ring model).
//!
//! This module contains the design sketches for the refactored Loop.
//! It is NOT connected to the main code path yet — it serves as
//! the architecture blueprint for the migration.
//!
//! ## Module map
//!
//! | Module              | Role                                   |
//! |---------------------|----------------------------------------|
//! | `types.rs`          | Shared types: Outcome, RingContext, CancelToken |
//! | `loop_core.rs`      | Loop dispatcher (thin, ~150L target)   |
//! | `engine_turn.rs`    | TurnEngine: gate→tools cycle           |
//! | `engine_tool.rs`    | ToolEngine: admit→execute→result       |
//! | `engine_session.rs` | SessionEngine: create/resume/reload    |
//! | `engine_input.rs`   | InputEngine: user input → turn start   |
//! | `engine_compact.rs` | CompactEngine: context summarization   |
//! | `engine_misc.rs`    | MiscEngine: undo/dashboard/mode/notify |
//!
//! ## Ring interface
//!
//! The central abstraction is the `Outcome` enum. Each Engine returns
//! an Outcome, and the Loop dispatcher acts on it:
//!
//! - `ContinueTurn` → re-enter TurnEngine for another gate lap
//! - `YieldToUser` → pause, wait for PermissionResponse or UserInput
//! - `TurnComplete` → emit Done, return to Idle
//! - `Handled` → return to Idle
//! - `Error` → emit error, return to Idle
//! - `Shutdown` → exit loop
//!
//! ## Migration strategy
//!
//! 1. Complete the design sketches in this module
//! 2. Implement types.rs + loop_core.rs as standalone (no dependencies on old Loop)
//! 3. Implement each Engine one at a time, testing in isolation
//! 4. Wire up `new_ipc()` and `run()` to match the old external API exactly
//! 5. Swap `lib.rs` to re-export from `new/` instead of old code
//! 6. Delete old `lib.rs` methods, `turn.rs`, `tool_exec.rs`, `permission.rs`,
//!    `compact.rs`, `conflict.rs` (keep `agent.rs`, `lifecycle.rs`, `notification.rs`,
//!    `toast_com.rs`, `dashboard.rs`, `logger.rs`, `util.rs`)

pub mod types;
pub mod engine;
pub mod loop_core;
pub mod engine_turn;
pub mod engine_tool;
pub mod engine_session;
pub mod engine_input;
pub mod engine_compact;
pub mod engine_misc;
