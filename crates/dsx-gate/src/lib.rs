//! dsx-gate: API proxy gateway — holds API keys, streams LLM responses,
//! applies output quality guards, and provides audit logging.

pub mod runner;
pub mod types;
pub mod openai_api;
