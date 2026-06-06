//! Provider registry — known providers and their endpoints.
//!
//! Architecture:
//!   Provider (e.g. DeepSeek) has 1..N Endpoints (e.g. OpenAI-compat, Anthropic-native).
//!   User selects (provider_id, endpoint_id) → protocol + base_url auto-fill.
//!   Model list is fetched from endpoint's /models URL at runtime.
//!
//! Backward compat: old provider_id "deepseek-openai"/"deepseek-anthropic" are
//! auto-migrated to provider_id="deepseek" + endpoint="openai"/"anthropic".

use dsx_types::{EndpointSpec, ProviderSpec};

// ── Static registry ──

fn deepseek() -> ProviderSpec {
    ProviderSpec {
        id: "deepseek".into(),
        display: "DeepSeek".into(),
        endpoints: vec![
            EndpointSpec {
                id: "openai".into(),
                display: "OpenAI-compatible".into(),
                protocol: "openai".into(),
                base_url: "https://api.deepseek.com".into(),
                default_model: "deepseek-v4-flash".into(),
                models_url: Some("https://api.deepseek.com".into()),
            },
            EndpointSpec {
                id: "anthropic".into(),
                display: "Anthropic-native".into(),
                protocol: "anthropic".into(),
                base_url: "https://api.deepseek.com/anthropic".into(),
                default_model: "deepseek-v4-pro".into(),
                models_url: Some("https://api.deepseek.com".into()),
            },
        ],
    }
}

fn providers() -> Vec<ProviderSpec> {
    vec![deepseek()]
}

// ── Lookup ──

pub fn all_providers() -> Vec<ProviderSpec> {
    providers()
}

pub fn find_provider(id: &str) -> Option<ProviderSpec> {
    providers().into_iter().find(|p| p.id == id)
}

pub fn find_endpoint(provider_id: &str, endpoint_id: &str) -> Option<EndpointSpec> {
    find_provider(provider_id)
        .and_then(|p| p.endpoints.into_iter().find(|e| e.id == endpoint_id))
}

/// Resolve the first endpoint of a provider. Returns (endpoint, provider) tuples
/// for the default first endpoint.
pub fn first_endpoint_for(provider_id: &str) -> Option<EndpointSpec> {
    find_provider(provider_id)
        .and_then(|p| p.endpoints.into_iter().next())
}

// ── Model discovery ──

/// Models URL for a given (provider, endpoint) pair.
pub fn models_url_for(provider_id: &str, endpoint_id: &str) -> Option<String> {
    let ep = find_endpoint(provider_id, endpoint_id)?;
    let base = ep.models_url.as_deref().unwrap_or(&ep.base_url);
    let stripped = base.trim_end_matches('/');
    Some(format!("{}/models", stripped))
}

/// Fetch model list from the /models endpoint (sync, ureq).
/// Falls back to [default_model] if the request fails.
pub fn fetch_models(provider_id: &str, endpoint_id: &str, api_key: &str) -> Vec<String> {
    let ep = match find_endpoint(provider_id, endpoint_id) {
        Some(e) => e,
        None => return vec![],
    };

    let url = match models_url_for(provider_id, endpoint_id) {
        Some(u) => u,
        None => return vec![ep.default_model.clone()],
    };

    let req = ureq::get(&url).set("Authorization", &format!("Bearer {}", api_key));

    match req.timeout(std::time::Duration::from_secs(10)).call() {
        Ok(resp) => {
            let body: Result<serde_json::Value, _> = resp.into_json();
            match body {
                Ok(v) => {
                    let models: Vec<String> = v["data"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| m["id"].as_str().map(String::from))
                                .filter(|id| !id.starts_with("deepseek-re"))
                                .collect()
                        })
                        .unwrap_or_default();
                    if models.is_empty() {
                        vec![ep.default_model.clone()]
                    } else {
                        models
                    }
                }
                Err(_) => vec![ep.default_model.clone()],
            }
        }
        Err(_) => vec![ep.default_model.clone()],
    }
}

/// Get the default model for a (provider, endpoint) pair.
pub fn default_model_for(provider_id: &str, endpoint_id: &str) -> String {
    find_endpoint(provider_id, endpoint_id)
        .map(|e| e.default_model.clone())
        .unwrap_or_else(|| "deepseek-v4-flash".into())
}

/// Resolve protocol string for a (provider, endpoint) pair.
pub fn protocol_for(provider_id: &str, endpoint_id: &str) -> String {
    find_endpoint(provider_id, endpoint_id)
        .map(|e| e.protocol.clone())
        .unwrap_or_else(|| "openai".into())
}

/// Resolve base_url for a (provider, endpoint) pair.
pub fn base_url_for(provider_id: &str, endpoint_id: &str) -> String {
    find_endpoint(provider_id, endpoint_id)
        .map(|e| e.base_url.clone())
        .unwrap_or_default()
}

// ── Backward compatibility ──

/// Migrate old provider_id ("deepseek-openai" / "deepseek-anthropic") to new
/// (provider_id, endpoint) pair.
pub fn migrate_provider_id(old_pid: &str) -> (String, String) {
    match old_pid {
        "deepseek-openai" => ("deepseek".into(), "openai".into()),
        "deepseek-anthropic" => ("deepseek".into(), "anthropic".into()),
        other => {
            if find_provider(other).is_some() {
                let ep = first_endpoint_for(other)
                    .map(|e| e.id.clone())
                    .unwrap_or_else(|| "openai".into());
                (other.to_string(), ep)
            } else {
                ("deepseek".into(), "openai".into())
            }
        }
    }
}
