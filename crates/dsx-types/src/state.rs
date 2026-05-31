use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebugLevel {
    Low,
    Medium,
    High,
}

impl Default for DebugLevel {
    fn default() -> Self { DebugLevel::Medium }
}

impl DebugLevel {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "low" => DebugLevel::Low,
            "high" => DebugLevel::High,
            _ => DebugLevel::Medium,
        }
    }
}
