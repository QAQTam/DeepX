use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DebugLevel {
    Low,
    #[default]
    Medium,
    High,
}

impl DebugLevel {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "low" => DebugLevel::Low,
            "high" => DebugLevel::High,
            _ => DebugLevel::Medium,
        }
    }
}
