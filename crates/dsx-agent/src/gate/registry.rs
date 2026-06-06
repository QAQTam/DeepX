//! Provider preset registry — hardcoded endpoint configurations.
//!
//! Each preset bundles protocol + base_url + model list so the user
//! selects a provider and everything else auto-fills.

use super::types::ProviderKind;

#[derive(Debug, Clone)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub display: &'static str,
    pub provider: &'static str, // "deepseek" | "custom"
    pub protocol: &'static str, // "openai" | "anthropic"
    pub base_url: &'static str,
    pub default_model: &'static str,
    pub models: &'static [&'static str],
}

const DEEPSEEK_MODELS: &[&str] = &["deepseek-v4-flash", "deepseek-v4-pro"];

const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        id: "deepseek-openai",
        display: "DeepSeek (OpenAI)",
        provider: "deepseek",
        protocol: "openai",
        base_url: "https://api.deepseek.com",
        default_model: "deepseek-v4-flash",
        models: DEEPSEEK_MODELS,
    },
    ProviderPreset {
        id: "deepseek-anthropic",
        display: "DeepSeek (Anthropic)",
        provider: "deepseek",
        protocol: "anthropic",
        base_url: "https://api.deepseek.com",
        default_model: "deepseek-v4-pro",
        models: DEEPSEEK_MODELS,
    },
    ProviderPreset {
        id: "custom",
        display: "自定义 (Custom)",
        provider: "custom",
        protocol: "openai",
        base_url: "",
        default_model: "",
        models: &[],
    },
];

pub fn all_presets() -> &'static [ProviderPreset] {
    PRESETS
}

pub fn find_preset(id: &str) -> Option<&'static ProviderPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Get the default model for a preset, or "deepseek-v4-flash" as universal fallback.
pub fn default_model_for(id: &str) -> &'static str {
    find_preset(id).map(|p| p.default_model).unwrap_or("deepseek-v4-flash")
}

/// Get the model list for a preset.
pub fn models_for(id: &str) -> &'static [&'static str] {
    find_preset(id).map(|p| p.models).unwrap_or(&[])
}

/// Resolve provider kind from preset
pub fn provider_kind_for(id: &str) -> ProviderKind {
    find_preset(id)
        .map(|p| ProviderKind::from_str(p.protocol))
        .unwrap_or(ProviderKind::OpenAi)
}
