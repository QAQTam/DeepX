//! dsx-hp: health platform, API proxy gateway, error sentinel, and process registry.

// ── Runner (exposes binary logic as library function) ──

pub mod runner;

// ── Local modules ──

pub mod config;

// ── Health-platform modules (B01: guardian core) ──

pub mod types;
pub mod registry;
pub mod ipc_traits;

// ── Health subsystems ──

pub mod liveness;

// ── Anthropic native API client ──

pub mod anthropic_api;
pub use anthropic_api::{GatewayConfig, StreamEvent};