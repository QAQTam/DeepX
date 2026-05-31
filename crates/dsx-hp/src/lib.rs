//! dsx-hp: health platform, API proxy gateway, error sentinel, and process registry.

// ── Runner (exposes binary logic as library function) ──

pub mod runner;

// ── Health-platform modules ──

pub mod types;
pub mod registry;

// ── Health subsystems ──

pub mod liveness;

// ── OpenAI native API client ──

pub mod openai_api;