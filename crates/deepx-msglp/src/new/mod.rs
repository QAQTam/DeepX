//! new/ — production Ring-architecture Loop (primary).
//!
//! This loop replaces the old monolithic `Loop` with a pluggable Engine model.
//! Each Engine implements the [`Engine`] trait; the Loop dispatches `Ui2Agent`
//! commands by iterating engines in a try-handle chain.
//!
//! ## Module map
//!
//! | Module              | Role                                   |
//! |---------------------|----------------------------------------|
//! | `types.rs`          | Shared types: Outcome, RingContext, CancelToken |
//! | `loop_core.rs`      | Loop dispatcher                        |
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
//! - `TurnAborted` → emit Cancelled + Done, return to Idle
//! - `Handled` → return to Idle
//! - `Error` → emit error, return to Idle
//! - `Shutdown` → exit loop
//!
//! ## Extension
//!
//! To add a new feature:
//! 1. Create a new struct implementing `Engine`
//! 2. Add `Box::new(YourEngine::new())` to the engines vec in `Loop::new_ipc()`
//! 3. Done — no changes to `Loop::dispatch()` needed

pub mod engine;
pub mod engine_compact;
pub mod engine_input;
pub mod engine_misc;
pub mod engine_session;
pub mod engine_tool;
pub mod engine_turn;
pub mod loop_core;
pub mod types;
