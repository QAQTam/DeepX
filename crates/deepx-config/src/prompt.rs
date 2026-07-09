//! System prompt — partitioned into `[SECTION]` blocks for LLM clarity.
//!
//! Each section lives in a standalone `prompts/*.md` file. At compile time,
//! the files are embedded via `include_str!()`. At runtime, the user may
//! override any section by placing a same-named file in
//! `<data_dir>/prompts/` (e.g. `~/.deepx/prompts/role.md` on Windows).
//!
//! The `[SESSION]` block supports `{{today}}`, `{{os_info}}`, and
//! `{{tools_info}}` placeholders that are filled at session creation time.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Cached OS info string (e.g. "Windows 11 Pro 24H2 26200.5518"). Set at startup.
pub static OS_INFO: OnceLock<String> = OnceLock::new();

/// Cached toolchain versions (e.g. "pwsh 7.4 | rustc 1.92 | python 3.12"). Set at startup.
pub static TOOLS_INFO: OnceLock<String> = OnceLock::new();

// ── Compile-time defaults (embedded from prompts/*.md) ──

const DEFAULT_THINK_MAX: &str = include_str!("../prompts/think_max.md");
const DEFAULT_ROLE: &str = include_str!("../prompts/role.md");
const DEFAULT_PROTOCOL: &str = include_str!("../prompts/protocol.md");
const DEFAULT_RULES: &str = include_str!("../prompts/rules.md");
const DEFAULT_SESSION: &str = include_str!("../prompts/session.md");

// ── Public access to SESSION prefix (kept for backward compat) ──

pub const SESSION_PREFIX: &str = "[SESSION]";

// ── User override directory ──

/// Returns the user prompts override directory, if it exists.
fn user_prompts_dir() -> Option<PathBuf> {
    let dir = deepx_types::platform::data_dir().join("prompts");
    if dir.is_dir() { Some(dir) } else { None }
}

/// Load a prompt section: user override takes priority, fallback to embedded default.
fn load_section(name: &str, default: &str) -> String {
    if let Some(dir) = user_prompts_dir() {
        let path = dir.join(name);
        if path.is_file() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                return content;
            }
        }
    }
    default.to_string()
}

// ── Assembly ──

pub fn full_system_prompt() -> String {
    let mut p = String::new();
    p.push_str(&load_section("think_max.md", DEFAULT_THINK_MAX));
    p.push_str("\n\n");
    p.push_str(&load_section("role.md", DEFAULT_ROLE));
    p.push_str("\n\n");
    p.push_str(&load_section("protocol.md", DEFAULT_PROTOCOL));
    p.push_str("\n\n");
    p.push_str(&load_section("rules.md", DEFAULT_RULES));
    p.push_str("\n\n");
    p.push_str(SESSION_PREFIX);
    p
}

/// System prompt with date and OS/tools info injected into the [SESSION] block.
/// [SESSION] is placed last so prefix cache is shared across sessions —
/// the static blocks (THINK_MAX/ROLE/PROTOCOL/RULES) occupy identical positions.
/// The date is captured once at session creation and never updated,
/// preserving LLM cache prefix stability across turns.
pub fn full_system_prompt_with_date(today: &str, os_info: &str) -> String {
    let tools = TOOLS_INFO.get().map(|s| s.as_str()).unwrap_or("");
    let session_template = load_section("session.md", DEFAULT_SESSION);
    let session = session_template
        .replace("{{today}}", today)
        .replace("{{os_info}}", os_info)
        .replace("{{tools_info}}", tools);

    let mut p = String::new();
    p.push_str(&load_section("think_max.md", DEFAULT_THINK_MAX));
    p.push_str("\n\n");
    p.push_str(&load_section("role.md", DEFAULT_ROLE));
    p.push_str("\n\n");
    p.push_str(&load_section("protocol.md", DEFAULT_PROTOCOL));
    p.push_str("\n\n");
    p.push_str(&load_section("rules.md", DEFAULT_RULES));
    p.push_str("\n\n");
    p.push_str(&session);
    p
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prompt_contains_sections() {
        let p = full_system_prompt();
        assert!(p.contains("[THINK_MAX]"), "missing THINK_MAX");
        assert!(p.contains("[ROLE]"), "missing ROLE");
        assert!(p.contains("[PROTOCOL]"), "missing PROTOCOL");
        assert!(p.contains("[RULES]"), "missing RULES");
        assert!(p.contains("[SESSION]"), "missing SESSION prefix");
    }

    #[test]
    fn dated_prompt_injects_placeholders() {
        let p = full_system_prompt_with_date("2026-07-07", "Windows 11");
        assert!(p.contains("2026-07-07"), "missing date injection");
        assert!(p.contains("Windows 11"), "missing OS injection");
        assert!(!p.contains("{{today}}"), "placeholder not replaced");
        assert!(!p.contains("{{os_info}}"), "placeholder not replaced");
        assert!(!p.contains("{{tools_info}}"), "placeholder not replaced");
    }

    #[test]
    fn dated_prompt_contains_all_sections() {
        let p = full_system_prompt_with_date("2026-07-07", "Windows 11");
        assert!(p.contains("[THINK_MAX]"));
        assert!(p.contains("[ROLE]"));
        assert!(p.contains("[PROTOCOL]"));
        assert!(p.contains("[RULES]"));
        assert!(p.contains("[SESSION]"));
    }

    #[test]
    fn prompt_starts_with_think_max() {
        let p = full_system_prompt();
        assert!(p.starts_with("[THINK_MAX]"), "prompt should start with THINK_MAX");
    }
}
