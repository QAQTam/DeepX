//! Agent Skills: progressive skill injection from ~/.config/dsx/skills/.
//!
//! Skills are directories containing a SKILL.md with YAML frontmatter
//! (name + description) and optional scripts/references/assets.
//!
//! Three-stage progressive disclosure:
//!   1. Discovery: name + description injected into system prompt once at startup
//!   2. Activation: full SKILL.md loaded into Layer 3 context when task matches
//!   3. Execution: bundled scripts/references loaded on demand via read_file

use std::path::PathBuf;
use std::sync::OnceLock;

/// Global skill registry — loaded once at startup.
static SKILL_REGISTRY: OnceLock<Vec<SkillMeta>> = OnceLock::new();

/// Ensure skills are loaded. Called once at startup.
pub fn init() {
    let _ = SKILL_REGISTRY.set(load_skill_meta());
}

/// Get the global skill list.
pub fn all() -> &'static [SkillMeta] {
    SKILL_REGISTRY.get().map(|v| v.as_slice()).unwrap_or(&[])
}

/// Metadata extracted from a SKILL.md frontmatter.
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub dir: PathBuf,
}

/// Scan `{data_dir}/skills/` for valid skill directories, return metadata list.
pub fn load_skill_meta() -> Vec<SkillMeta> {
    let skills_dir = dsx_types::platform::data_dir().join("skills");
    if !skills_dir.is_dir() {
        return Vec::new();
    }

    let mut skills = Vec::new();
    for entry in std::fs::read_dir(&skills_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        let content = match std::fs::read_to_string(&skill_md) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let (name, description) = match parse_frontmatter(&content) {
            Some(v) => v,
            None => continue,
        };
        skills.push(SkillMeta { name, description, dir: path });
    }
    skills
}

/// Build the skills section of the system prompt.
pub fn skills_prompt_section() -> String {
    let skills = all();
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n### Available Skills\n\n");
    s.push_str("Skills provide domain-specific instructions. When a task matches a skill's description, activate it.\n\n");
    for sk in skills {
        s.push_str(&format!("- {}: {}\n", sk.name, sk.description));
    }
    s.push('\n');
    s
}

/// Activate a skill: read full SKILL.md and inject into Layer 3 context.
pub fn activate_skill(state: &mut crate::agent::AgentState, name: &str) -> bool {
    let skills_dir = dsx_types::platform::data_dir().join("skills");
    let skill_md = skills_dir.join(name).join("SKILL.md");
    let content = match std::fs::read_to_string(&skill_md) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let label = format!("skill:{}", name);
    state.turn_annotations.push(format!("[{}]\n{}", label, content));
    true
}

/// Auto-detect relevant skills from user input and activate them.
/// Simple keyword overlap: each skill's name + description is scored
/// against the user's text. Skills above threshold are injected into context.
pub fn auto_activate(state: &mut crate::agent::AgentState, user_text: &str) {
    let text_lower = user_text.to_lowercase();
    let candidates: Vec<(String, String)> = all().iter()
        .map(|s| (s.name.clone(), format!("{} {}", s.name, s.description).to_lowercase()))
        .collect();

    for (name, haystack) in &candidates {
        let words: Vec<&str> = haystack.split_whitespace().collect();
        let matches = words.iter().filter(|w| text_lower.contains(*w)).count();

        if matches >= 2 || text_lower.contains(name.as_str()) {
            if crate::skills::activate_skill(state, name) {
                log::info!("dsx: auto-activated skill '{}' ({} keyword matches)", name, matches);
            }
        }
    }
}

/// Parse YAML frontmatter from SKILL.md content.
/// Returns (name, description) or None if invalid.
fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let body = content.trim();
    if !body.starts_with("---\n") && !body.starts_with("---\r\n") {
        return None;
    }
    let rest = &body[3..].trim_start_matches(|c| c == '\r');
    let end = rest.find("\n---")?;
    let fm = &rest[..end];

    let mut name = String::new();
    let mut desc = String::new();
    let mut in_metadata = false;

    for line in fm.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("metadata:") {
            in_metadata = true;
            continue;
        }
        if in_metadata {
            if line.starts_with("  ") || line.starts_with('\t') {
                continue;
            } else {
                in_metadata = false;
            }
        }
        if let Some(v) = trimmed.strip_prefix("name:") {
            name = v.trim().to_string();
        } else if let Some(v) = trimmed.strip_prefix("description:") {
            desc = v.trim().to_string();
        }
    }

    if name.is_empty() || desc.is_empty() {
        return None;
    }
    Some((name, desc))
}
