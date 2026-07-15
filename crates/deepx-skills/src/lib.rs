//! Agent Skill 运行时：发现、解析、目录渲染和完整激活。
//!
//! Catalog 只暴露元数据；正文仅在显式提及或 `skills(action=activate)` 时读取。

use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const ACTIVATION_MARKER: &str = "[DEEPX_SKILL_V1]";
pub const MAX_SKILL_BYTES: u64 = 512 * 1024;
const MAX_SCAN_DEPTH: usize = 6;
const MAX_SCANNED_DIRS: usize = 2_000;
const MAX_SKILLS: usize = 256;
const MAX_RESOURCES: usize = 200;
const MAX_CATALOG_CHARS: usize = 8_000;
const MAX_NAME_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 1_024;
const MAX_COMPATIBILITY_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillScope {
    Project,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub metadata: BTreeMap<String, String>,
    /// Agent Skills experimental field. It never grants permissions by itself;
    /// DeepX always intersects it with the active permission policy.
    pub allowed_tools: Vec<String>,
    pub path: PathBuf,
    pub scope: SkillScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDiagnostic {
    pub path: PathBuf,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillCatalog {
    pub skills: Vec<SkillMetadata>,
    pub diagnostics: Vec<SkillDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillActivation {
    pub metadata: SkillMetadata,
    pub body: String,
    pub resources: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillResource {
    pub skill_name: String,
    pub relative_path: PathBuf,
    pub absolute_path: PathBuf,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct Frontmatter {
    name: String,
    description: String,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    compatibility: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(default, rename = "allowed-tools")]
    allowed_tools: Option<String>,
}

#[derive(Debug)]
struct ParsedMetadata {
    metadata: SkillMetadata,
    warnings: Vec<String>,
}

/// 动态发现当前工作区和用户目录中的 skills。
///
/// 优先级固定为：项目 `.deepx` > `.agents` > `skills`，随后是用户
/// `.deepx` > `.agents`。同名 skill 只保留优先级最高者。
pub fn discover(workspace: &Path) -> SkillCatalog {
    let workspace = absolutize(workspace);
    let mut roots = vec![
        (workspace.join(".deepx/skills"), SkillScope::Project),
        (workspace.join(".agents/skills"), SkillScope::Project),
        (workspace.join("skills"), SkillScope::Project),
    ];
    if let Some(home) = home_dir() {
        roots.push((home.join(".deepx/skills"), SkillScope::User));
        roots.push((home.join(".agents/skills"), SkillScope::User));
    }
    discover_roots(&roots)
}

fn discover_roots(roots: &[(PathBuf, SkillScope)]) -> SkillCatalog {
    let mut catalog = SkillCatalog::default();
    let mut seen_names: HashMap<String, PathBuf> = HashMap::new();
    let mut scanned_dirs = 0usize;

    for (root, scope) in roots {
        scan_root(
            root,
            *scope,
            &mut catalog,
            &mut seen_names,
            &mut scanned_dirs,
        );
        if scanned_dirs >= MAX_SCANNED_DIRS || catalog.skills.len() >= MAX_SKILLS {
            break;
        }
    }
    catalog
}

fn scan_root(
    root: &Path,
    scope: SkillScope,
    catalog: &mut SkillCatalog,
    seen_names: &mut HashMap<String, PathBuf>,
    scanned_dirs: &mut usize,
) {
    if !root.is_dir() {
        return;
    }
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if *scanned_dirs >= MAX_SCANNED_DIRS || catalog.skills.len() >= MAX_SKILLS {
            return;
        }
        *scanned_dirs += 1;

        let mut entries = match fs::read_dir(&dir) {
            Ok(entries) => entries.flatten().collect::<Vec<_>>(),
            Err(error) => {
                catalog.diagnostics.push(SkillDiagnostic {
                    path: dir,
                    severity: DiagnosticSeverity::Error,
                    message: format!("cannot scan directory: {error}"),
                });
                continue;
            }
        };
        entries.sort_by_key(|entry| entry.file_name());
        entries.reverse();

        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if !file_type.is_dir() || file_type.is_symlink() || should_skip_dir(&name) {
                continue;
            }

            let skill_path = path.join("SKILL.md");
            if skill_path.is_file() {
                match parse_metadata(&skill_path, scope) {
                    Ok(parsed) => {
                        let skill = parsed.metadata;
                        for message in parsed.warnings {
                            catalog.diagnostics.push(SkillDiagnostic {
                                path: skill.path.clone(),
                                severity: DiagnosticSeverity::Warning,
                                message,
                            });
                        }
                        if let Some(selected) = seen_names.get(&skill.name) {
                            catalog.diagnostics.push(SkillDiagnostic {
                                path: skill.path.clone(),
                                severity: DiagnosticSeverity::Warning,
                                message: format!(
                                    "skill '{}' shadowed by {}",
                                    skill.name,
                                    selected.display()
                                ),
                            });
                        } else {
                            seen_names.insert(skill.name.clone(), skill.path.clone());
                            catalog.skills.push(skill);
                        }
                    }
                    Err(message) => catalog.diagnostics.push(SkillDiagnostic {
                        path: skill_path,
                        severity: DiagnosticSeverity::Error,
                        message,
                    }),
                }
            }
            if depth < MAX_SCAN_DEPTH {
                stack.push((path, depth + 1));
            }
        }
    }
}

fn should_skip_dir(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target" | ".git")
}

fn parse_metadata(path: &Path, scope: SkillScope) -> Result<ParsedMetadata, String> {
    let raw = read_bounded(path)?;
    let (frontmatter, _) = split_skill_file(&raw)?;
    let parsed = parse_frontmatter(frontmatter)?;
    let name = parsed.name.trim().to_string();
    let description = parsed.description.trim().to_string();
    if description.is_empty() {
        return Err("missing skill description".into());
    }
    let mut warnings =
        validate_standard_fields(path, &name, &description, parsed.compatibility.as_deref());
    if name.is_empty() {
        return Err("missing skill name".into());
    }
    if let Some(parent_name) = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        && parent_name != name
    {
        warnings.push(format!(
            "name '{name}' does not match parent directory '{parent_name}'"
        ));
    }
    Ok(ParsedMetadata {
        metadata: SkillMetadata {
            name,
            description,
            license: parsed
                .license
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            compatibility: parsed
                .compatibility
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            metadata: parsed.metadata,
            allowed_tools: parsed
                .allowed_tools
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            path: absolutize(path),
            scope,
        },
        warnings,
    })
}

fn validate_standard_fields(
    _path: &Path,
    name: &str,
    description: &str,
    compatibility: Option<&str>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let name_chars = name.chars().count();
    if name_chars == 0 || name_chars > MAX_NAME_CHARS {
        warnings.push(format!("name must contain 1-{MAX_NAME_CHARS} characters"));
    }
    let valid_chars = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid_chars || name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        warnings.push(
            "name is not Agent Skills compliant (lowercase letters, digits, single hyphens only)"
                .into(),
        );
    }
    let description_chars = description.chars().count();
    if description_chars == 0 || description_chars > MAX_DESCRIPTION_CHARS {
        warnings.push(format!(
            "description must contain 1-{MAX_DESCRIPTION_CHARS} characters"
        ));
    }
    if compatibility.is_some_and(|value| value.chars().count() > MAX_COMPATIBILITY_CHARS) {
        warnings.push(format!(
            "compatibility exceeds {MAX_COMPATIBILITY_CHARS} characters"
        ));
    }
    warnings
}

fn parse_frontmatter(frontmatter: &str) -> Result<Frontmatter, String> {
    serde_yaml::from_str(frontmatter).map_err(|error| format!("invalid YAML frontmatter: {error}"))
}

/// Parse the YAML frontmatter boundary. Only `---\n` start is accepted;
/// legacy `[SKILLS]` prefix and any other content before the frontmatter
/// are rejected so that incompatible skills are skipped.
fn split_skill_file(raw: &str) -> Result<(&str, &str), String> {
    let normalized = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if !normalized.starts_with("---\n") {
        return Err("SKILL.md must start with YAML frontmatter (---)".into());
    }
    let start = 0;
    let after_open = start + 4;
    let remainder = normalized
        .get(after_open..)
        .ok_or("invalid YAML frontmatter opening boundary")?;
    let close = remainder
        .find("\n---\n")
        .or_else(|| remainder.strip_suffix("\n---").map(|body| body.len()))
        .ok_or("unclosed YAML frontmatter")?;
    let body_start = after_open + close + 5;
    let body = normalized.get(body_start..).unwrap_or("").trim();
    let frontmatter = remainder
        .get(..close)
        .ok_or("invalid YAML frontmatter closing boundary")?;
    Ok((frontmatter, body))
}

fn read_bounded(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("cannot stat file: {error}"))?;
    if metadata.len() > MAX_SKILL_BYTES {
        return Err(format!("skill file exceeds {} bytes", MAX_SKILL_BYTES));
    }
    fs::read_to_string(path)
        .map(|text| text.replace("\r\n", "\n").replace('\r', "\n"))
        .map_err(|error| format!("cannot read skill: {error}"))
}

pub fn load_named(workspace: &Path, name: &str) -> Result<SkillActivation, String> {
    let catalog = discover(workspace);
    let metadata = catalog
        .skills
        .into_iter()
        .find(|skill| skill.name == name)
        .ok_or_else(|| format!("unknown skill '{name}'"))?;
    load(&metadata)
}

/// Validate one SKILL.md against the portable Agent Skills specification.
/// Compatibility parsing remains available during discovery, but every
/// standards violation is returned as an error here.
pub fn validate_file(path: &Path) -> Vec<SkillDiagnostic> {
    match parse_metadata(path, SkillScope::Project) {
        Ok(parsed) => parsed
            .warnings
            .into_iter()
            .map(|message| SkillDiagnostic {
                path: absolutize(path),
                severity: DiagnosticSeverity::Error,
                message,
            })
            .collect(),
        Err(message) => vec![SkillDiagnostic {
            path: absolutize(path),
            severity: DiagnosticSeverity::Error,
            message,
        }],
    }
}

pub fn load(metadata: &SkillMetadata) -> Result<SkillActivation, String> {
    let raw = read_bounded(&metadata.path)?;
    let (_, body) = split_skill_file(&raw)?;
    let dir = metadata.path.parent().unwrap_or(Path::new("."));
    Ok(SkillActivation {
        metadata: metadata.clone(),
        body: body.to_string(),
        resources: list_resources(dir),
    })
}

/// Read a bundled text resource while enforcing containment in the selected
/// skill directory. Absolute paths, parent traversal, directories, symlink
/// escapes, binary data, and oversized files are rejected.
pub fn read_resource(
    workspace: &Path,
    skill_name: &str,
    relative_path: &Path,
) -> Result<SkillResource, String> {
    if relative_path.as_os_str().is_empty()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("resource path must be a non-empty relative path without '..'".into());
    }
    let activation = load_named(workspace, skill_name)?;
    let root = activation
        .metadata
        .path
        .parent()
        .ok_or("skill has no parent directory")?
        .canonicalize()
        .map_err(|error| format!("cannot resolve skill directory: {error}"))?;
    let candidate = root.join(relative_path);
    let resolved = candidate
        .canonicalize()
        .map_err(|error| format!("cannot resolve skill resource: {error}"))?;
    if !resolved.starts_with(&root) {
        return Err("skill resource escapes its skill directory".into());
    }
    if !resolved.is_file() {
        return Err("skill resource is not a file".into());
    }
    let content = read_bounded(&resolved)?;
    Ok(SkillResource {
        skill_name: activation.metadata.name,
        relative_path: relative_path.to_path_buf(),
        absolute_path: resolved,
        content,
    })
}

fn list_resources(root: &Path) -> Vec<PathBuf> {
    let mut resources = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match fs::read_dir(&dir) {
            Ok(entries) => entries.flatten().collect::<Vec<_>>(),
            Err(_) => continue,
        };
        entries.sort_by_key(|entry| entry.file_name());
        entries.reverse();
        for entry in entries {
            if resources.len() >= MAX_RESOURCES {
                return resources;
            }
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                if !should_skip_dir(&entry.file_name().to_string_lossy()) {
                    stack.push(path);
                }
            } else if entry.file_name() != "SKILL.md"
                && let Ok(relative) = path.strip_prefix(root)
            {
                resources.push(relative.to_path_buf());
            }
        }
    }
    resources.sort();
    resources
}

pub fn render_catalog(catalog: &SkillCatalog) -> String {
    if catalog.skills.is_empty() {
        return String::new();
    }
    let mut output = String::from(
        "Skills use progressive disclosure. Match a task against name and description only; do not infer the body. For an implicit match, call `skills` with action=`activate` and the exact name before acting. A user `$skill-name` mention is injected by the host directly. Load bundled files on demand with `skills` action=`resource`, the same name, and a manifest-relative path; do not use generic `read` for skill resources. Use action=`list` only for catalog diagnostics and action=`validate` only for strict portability checks.\n\n<available_skills>\n",
    );
    let mut omitted = 0usize;
    for skill in &catalog.skills {
        let description: String = skill.description.chars().take(600).collect();
        let line = format!(
            "- {}: {}\n",
            skill.name,
            description.replace(['\r', '\n'], " "),
        );
        if output.chars().count() + line.chars().count() > MAX_CATALOG_CHARS {
            omitted += 1;
        } else {
            output.push_str(&line);
        }
    }
    if omitted > 0 {
        output.push_str(&format!(
            "- ... {omitted} skills omitted by catalog budget\n"
        ));
    }
    output.push_str("</available_skills>");
    output
}

/// Render a numbered catalog for use as a stable system message.
/// Skills are sorted by name for deterministic numbering.
pub fn render_catalog_numbered(skills: &[SkillMetadata]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<&SkillMetadata> = skills.iter().collect();
    sorted.sort_by_key(|s| &s.name);

    let mut output = String::from(
        "Available skills (use `$S{N}` or `skills(action=activate, name=\"...\")` to load):\n\n",
    );
    for (i, skill) in sorted.iter().enumerate() {
        let desc: String = skill
            .description
            .chars()
            .take(500)
            .collect::<String>()
            .replace(['\r', '\n'], " ");
        output.push_str(&format!("S{}: {} — {}\n", i + 1, skill.name, desc));
    }
    output
}

pub fn render_activation(activation: &SkillActivation) -> String {
    let dir = activation.metadata.path.parent().unwrap_or(Path::new("."));
    let mut output = format!(
        "{ACTIVATION_MARKER}\nname: {}\nsource: {}\ndirectory: {}\nRelative paths in these instructions are resolved from this directory.\n\n--- instructions ---\n{}",
        activation.metadata.name,
        activation.metadata.path.display(),
        dir.display(),
        activation.body,
    );
    if let Some(compatibility) = &activation.metadata.compatibility {
        output.push_str("\n\n--- compatibility ---\n");
        output.push_str(compatibility);
    }
    if !activation.metadata.allowed_tools.is_empty() {
        output.push_str("\n\n--- requested tools (permission policy still applies) ---\n");
        output.push_str(&activation.metadata.allowed_tools.join(" "));
    }
    if !activation.resources.is_empty() {
        output.push_str("\n\n--- bundled resources (load on demand) ---\n");
        for resource in &activation.resources {
            output.push_str("- ");
            output.push_str(&resource.display().to_string());
            output.push('\n');
        }
    }
    output.push_str("\n[END_DEEPX_SKILL_V1]");
    output
}

pub fn is_activation_text(text: &str) -> bool {
    text.starts_with(ACTIVATION_MARKER) && text.contains("[END_DEEPX_SKILL_V1]")
}

pub fn activation_name(text: &str) -> Option<&str> {
    if !is_activation_text(text) {
        return None;
    }
    text.lines().find_map(|line| line.strip_prefix("name: "))
}

pub fn explicit_mentions(text: &str, catalog: &SkillCatalog) -> Vec<SkillMetadata> {
    let available = catalog
        .skills
        .iter()
        .map(|skill| (skill.name.as_str(), skill))
        .collect::<HashMap<_, _>>();
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'$' {
            index += 1;
            continue;
        }
        let start = index + 1;
        let mut end = start;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || matches!(bytes[end], b'-' | b'_'))
        {
            end += 1;
        }
        if end > start
            && let Some(mention) = text.get(start..end)
            && let Some(skill) = available.get(mention)
            && seen.insert(skill.name.clone())
        {
            found.push((*skill).clone());
        }
        index = end.max(index + 1);
    }
    found
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn write_skill(root: &Path, dir: &str, content: &str) -> PathBuf {
        let skill_dir = root.join(dir);
        fs::create_dir_all(&skill_dir).unwrap();
        let path = skill_dir.join("SKILL.md");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn rejects_legacy_skills_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_skill(
            &temp.path().join("skills"),
            "deepx/debug",
            "[SkILLS]\n---\nname: deepx-debug\ndescription: Debug failures.\n---\n\n# Full body",
        );
        // Legacy [SKILLS] prefix is no longer accepted — the skill must be skipped.
        assert!(parse_metadata(&path, SkillScope::Project).is_err());
    }

    #[test]
    fn accepts_windows_line_endings() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_skill(
            temp.path(),
            "windows",
            "---\r\nname: windows\r\ndescription: Windows lines.\r\n---\r\n\r\nFull body",
        );
        let metadata = parse_metadata(&path, SkillScope::Project).unwrap().metadata;
        assert_eq!(load(&metadata).unwrap().body, "Full body");
    }

    #[test]
    fn project_precedence_is_deterministic() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        write_skill(
            &first,
            "same",
            "---\nname: same\ndescription: First.\n---\n\nOne",
        );
        write_skill(
            &second,
            "same",
            "---\nname: same\ndescription: Second.\n---\n\nTwo",
        );
        let catalog = discover_roots(&[(first, SkillScope::Project), (second, SkillScope::User)]);
        assert_eq!(catalog.skills.len(), 1);
        assert_eq!(catalog.skills[0].description, "First.");
        assert_eq!(catalog.diagnostics.len(), 1);
    }

    #[test]
    fn activation_is_full_and_lists_resources() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("skills");
        let path = write_skill(
            &root,
            "large",
            &format!(
                "---\nname: large\ndescription: Large skill.\n---\n\n{}",
                "instruction\n".repeat(350)
            ),
        );
        fs::create_dir_all(path.parent().unwrap().join("references")).unwrap();
        fs::write(path.parent().unwrap().join("references/info.md"), "info").unwrap();
        let metadata = parse_metadata(&path, SkillScope::Project).unwrap().metadata;
        let activation = load(&metadata).unwrap();
        let rendered = render_activation(&activation);
        assert!(rendered.matches("instruction\n").count() >= 350);
        assert!(
            rendered.contains("references\\info.md") || rendered.contains("references/info.md")
        );
        assert!(is_activation_text(&rendered));
    }

    #[test]
    fn extracts_only_known_explicit_mentions_once() {
        let catalog = SkillCatalog {
            skills: vec![SkillMetadata {
                name: "review-code".into(),
                description: "Review code.".into(),
                license: None,
                compatibility: None,
                metadata: BTreeMap::new(),
                allowed_tools: Vec::new(),
                path: PathBuf::from("review/SKILL.md"),
                scope: SkillScope::Project,
            }],
            diagnostics: Vec::new(),
        };
        let found = explicit_mentions("use $review-code and $missing then $review-code", &catalog);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "review-code");
    }

    #[test]
    fn parses_standard_optional_frontmatter() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_skill(
            temp.path(),
            "portable",
            "---\nname: portable\ndescription: Use when testing portable skills.\nlicense: MIT\ncompatibility: Requires git.\nmetadata:\n  author: deepx\n  version: \"1\"\nallowed-tools: read exec_run\n---\n\nBody",
        );
        let parsed = parse_metadata(&path, SkillScope::Project).unwrap();
        assert!(parsed.warnings.is_empty());
        assert_eq!(parsed.metadata.license.as_deref(), Some("MIT"));
        assert_eq!(
            parsed.metadata.compatibility.as_deref(),
            Some("Requires git.")
        );
        assert_eq!(
            parsed.metadata.metadata.get("author").map(String::as_str),
            Some("deepx")
        );
        assert_eq!(parsed.metadata.allowed_tools, vec!["read", "exec_run"]);
        assert!(validate_file(&path).is_empty());
    }

    #[test]
    fn strict_validation_reports_nonportable_name_and_directory_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_skill(
            temp.path(),
            "actual-dir",
            "---\nname: Bad_Name\ndescription: Compatibility discovery still loads this.\n---\n\nBody",
        );
        let parsed = parse_metadata(&path, SkillScope::Project).unwrap();
        assert_eq!(parsed.metadata.name, "Bad_Name");
        let diagnostics = validate_file(&path);
        assert!(
            diagnostics
                .iter()
                .all(|item| item.severity == DiagnosticSeverity::Error)
        );
        assert!(
            diagnostics
                .iter()
                .any(|item| item.message.contains("not Agent Skills compliant"))
        );
        assert!(
            diagnostics
                .iter()
                .any(|item| item.message.contains("does not match parent"))
        );
    }

    #[test]
    fn invalid_yaml_frontmatter_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let path = write_skill(
            temp.path(),
            "fallback",
            "---\nname: fallback\ndescription: [invalid yaml\n---\n\nBody",
        );
        // Invalid YAML is no longer accepted via compatibility parser.
        assert!(parse_metadata(&path, SkillScope::Project).is_err());
    }

    #[test]
    fn resource_reads_are_contained_and_complete() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path();
        let skill_path = write_skill(
            &workspace.join(".agents/skills"),
            "resource-skill",
            "---\nname: resource-skill\ndescription: Use for resource tests.\n---\n\nRead references/info.md",
        );
        fs::create_dir_all(skill_path.parent().unwrap().join("references")).unwrap();
        fs::write(
            skill_path.parent().unwrap().join("references/info.md"),
            "complete resource",
        )
        .unwrap();
        let resource =
            read_resource(workspace, "resource-skill", Path::new("references/info.md")).unwrap();
        assert_eq!(resource.content, "complete resource");
        assert!(read_resource(workspace, "resource-skill", Path::new("../outside.md")).is_err());
        assert!(read_resource(workspace, "resource-skill", Path::new("references")).is_err());
    }

    #[test]
    fn render_catalog_numbered_assigns_stable_ids() {
        let meta = |name: &str, desc: &str| SkillMetadata {
            name: name.into(),
            description: desc.into(),
            license: None,
            compatibility: None,
            metadata: BTreeMap::new(),
            allowed_tools: vec![],
            path: PathBuf::from(name),
            scope: SkillScope::Project,
        };
        let skills = vec![
            meta("zebra", "Last in alphabetical order"),
            meta("alpha", "First in alphabetical order"),
        ];
        assert_eq!(
            render_catalog_numbered(&skills),
            "Available skills (use `$S{N}` or `skills(action=activate, name=\"...\")` to load):\n\nS1: alpha — First in alphabetical order\nS2: zebra — Last in alphabetical order\n"
        );
    }

    #[test]
    fn render_catalog_numbered_empty_returns_empty() {
        assert_eq!(render_catalog_numbered(&[]), "");
    }
}
