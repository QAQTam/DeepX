//! dsx-agent: central agent process — orchestrator, memory, session, context.

// ── Runner (exposes binary logic as library function) ──

pub mod runner;

pub mod config;
pub mod assembly;
pub mod dsx_log;
pub mod prompt;
// ── Core modules ──
pub mod agent;
pub mod health;
pub mod tools;

pub mod orchestrator;
pub mod session;
pub mod tool_parser;
pub mod hp;
