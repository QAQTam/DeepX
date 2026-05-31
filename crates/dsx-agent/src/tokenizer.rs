//! Token counting with DeepSeek tokenizer and heuristic fallback.
//!
//! Loads the DeepSeek V3 tokenizer from `{data_dir}/tokenizer.json` at
//! runtime. Falls back to a CJK-aware heuristic if the file is not found.
//!
//! `TokenBreakdown` is re-exported from `dsx-types` for caller convenience.

use dsx_types::Message;
pub use dsx_types::token::TokenBreakdown;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokenizers::Tokenizer;

// ── Lazy tokenizer ──

static DEEPSEEK_TOKENIZER: OnceLock<Option<Tokenizer>> = OnceLock::new();

fn tokenizer_path() -> PathBuf {
    // Prefer DSX_TOKENIZER_PATH env var, then data_dir/tokenizer.json
    if let Ok(p) = std::env::var("DSX_TOKENIZER_PATH") {
        return PathBuf::from(p);
    }
    dsx_types::platform::data_dir().join("tokenizer.json")
}

fn get_tokenizer() -> Option<&'static Tokenizer> {
    DEEPSEEK_TOKENIZER.get_or_init(|| {
        let path = tokenizer_path();
        match Tokenizer::from_file(&path) {
            Ok(tok) => {
                log::info!("dsx: DeepSeek tokenizer loaded from {}", path.display());
                Some(tok)
            }
            Err(e) => {
                log::warn!("dsx: tokenizer not found at {} ({}) — using heuristic", path.display(), e);
                None
            }
        }
    }).as_ref()
}

pub fn count_tokens(text: &str) -> u32 {
    if let Some(tok) = get_tokenizer() {
        tok.encode(text.to_string(), false).map(|e| e.get_ids().len() as u32).unwrap_or_else(|_| heuristic_count(text))
    } else { heuristic_count(text) }
}

fn heuristic_count(text: &str) -> u32 {
    let cjk: usize = text.chars().filter(|c| matches!(c,
        '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' |
        '\u{3000}'..='\u{303f}' | '\u{ff00}'..='\u{ffef}' |
        '\u{3040}'..='\u{30ff}'
    )).count();
    (text.len().saturating_sub(cjk) as f64 / 3.3 + cjk as f64 / 1.67) as u32
}

// ── Message counting ──

pub fn count_message_tokens(msg: &Message) -> u32 {
    let mut t = 4u32;
    for block in &msg.content {
        match block {
            dsx_types::ContentBlock::Text { text } => {
                t += count_tokens(text);
            }
            dsx_types::ContentBlock::Reasoning { reasoning, .. } => {
                t += count_tokens(reasoning);
            }
            dsx_types::ContentBlock::ToolUse { name, input, .. } => {
                t += count_tokens(name);
                t += count_tokens(&input.to_string());
                t += 8;
            }
            dsx_types::ContentBlock::ToolResult { content, .. } => {
                t += count_tokens(content);
                t += 2;
            }
        }
    }
    t
}

pub fn estimate_messages_tokens(messages: &[Message]) -> u32 {
    messages.iter().map(count_message_tokens).sum()
}

// ── TokenCount trait impl for Message ──
//
// Uses the DeepSeek tokenizer when available, falling back to the
// heuristic in dsx-types. The orphan rule applies here, so we wrap
// Message in a newtype. However, dsx-types already provides blanket
// impls via `count_tokens` methods on Message. We just use those.
// See dsx-types::token::TokenCount for the trait definition.

