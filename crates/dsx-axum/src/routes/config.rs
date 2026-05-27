//! GET/PUT /api/config — Read/write DeepX configuration.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Serialize)]
pub struct ConfigResponse {
    pub ready: bool,
    pub base_url: String,
    pub model: String,
    pub effort: Option<String>,
    pub max_tokens: u32,
    pub context_limit: u32,
    pub auto_mode: bool,
}

#[derive(Deserialize)]
pub struct ConfigUpdate {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub max_tokens: Option<u32>,
    pub context_limit: Option<u32>,
}

/// GET /api/config
pub async fn get_config(State(state): State<Arc<AppState>>) -> Json<ConfigResponse> {
    let c = state.config.read().unwrap();
    Json(ConfigResponse {
        ready: c.is_ready(),
        base_url: c.base_url.clone(),
        model: c.model.clone(),
        effort: c.effort.clone(),
        max_tokens: c.max_tokens,
        context_limit: c.context_limit,
        auto_mode: c.auto_mode,
    })
}

/// PUT /api/config — Partial update. Saves to disk AND updates in-memory config.
/// Agent loop reads config fresh on every turn, so changes take effect immediately.
pub async fn put_config(
    State(state): State<Arc<AppState>>,
    Json(update): Json<ConfigUpdate>,
) -> Result<Json<ConfigResponse>, StatusCode> {
    let mut cfg = state.config.write().unwrap();

    if let Some(key) = &update.api_key {
        if !key.is_empty() {
            cfg.api_key = key.clone();
        }
    }
    if let Some(url) = &update.base_url { cfg.base_url = url.clone(); }
    if let Some(m) = &update.model { cfg.model = m.clone(); }
    if let Some(e) = &update.effort { cfg.effort = Some(e.clone()); }
    if let Some(mt) = update.max_tokens { cfg.max_tokens = mt; }
    if let Some(cl) = update.context_limit { cfg.context_limit = cl; }

    // If API key changed, restart HP daemon so it picks up the new key
    if update.api_key.is_some() {
        dsx_agent::hp::kill_hp_daemon();
        // HP will be restarted automatically on next chat turn via try_reconnect()
    }

    cfg.save();

    Ok(Json(ConfigResponse {
        ready: cfg.is_ready(),
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        effort: cfg.effort.clone(),
        max_tokens: cfg.max_tokens,
        context_limit: cfg.context_limit,
        auto_mode: cfg.auto_mode,
    }))
}
