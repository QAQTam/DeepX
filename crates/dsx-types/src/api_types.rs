use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ── Usage ──

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    #[serde(default)]
    pub prompt_cache_hit_tokens: u32,
    #[serde(default)]
    pub prompt_cache_miss_tokens: u32,
}
