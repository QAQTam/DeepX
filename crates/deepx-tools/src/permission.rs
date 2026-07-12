//! Permission engine: tool categories, permission levels, and trusted folder management.
//!
//! ## Architecture
//! - `ToolCategory` classifies every tool by risk profile (Read/Write/Exec/Net).
//! - `PermissionLevel` defines the default policy (1–4).
//! - `needs_permission()` evaluates whether a tool call requires user confirmation.
//! - `TrustedFolderSet` persists cross-workspace folder trust decisions.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ──────────────────────────────────────
// Tool category taxonomy
// ──────────────────────────────────────

/// Risk profile for each tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    /// No side effects: file_read, explore, search, list, git_diff/log/show/status,
    /// memory_read, plan_list, process_check.
    Read,
    /// Mutates files or state: file_write/edit/delete/move/copy, git_add/commit,
    /// memory_write/clear, plan_create/submit, task_create/update/delete.
    Write,
    /// Executes arbitrary code: exec_run, spawn_subagent.
    Exec,
    /// Outbound network: web_fetch, web_search, context7_query.
    Net,
}

/// Classify a tool by its registered name.
pub fn categorize_tool(name: &str) -> ToolCategory {
    match name {
        // ── Read ──
        "read" | "list" | "search" | "diff"
        | "explore_scan"
        | "git_diff" | "git_log" | "git_show" | "git_status"
        | "memory_read" | "plan_list" | "plan_submit"
        | "process_check" | "process_wait"
        | "context7"
        => ToolCategory::Read,

        // ── Write ──
        "write" | "edit" | "edit_block" | "delete"
        | "git_add" | "git_commit"
        | "git_branch" | "git_checkout" | "git_merge" | "git_restore"
        | "memory_write" | "memory_clear"
        | "plan_create"
        | "task_create" | "task_update" | "task_delete"
        => ToolCategory::Write,

        // ── Exec ──
        "exec_run" | "spawn_subagent" => ToolCategory::Exec,

        // ── Net ──
        "web" => ToolCategory::Net,

        // Unknown tools default to Write (conservative: assume mutation).
        _ => ToolCategory::Write,
    }
}

// ──────────────────────────────────────
// Permission level
// ──────────────────────────────────────

/// Agent operating permission level (1–4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum PermissionLevel {
    /// Level 1: Every tool call requires user confirmation.
    MaxLockdown = 1,
    /// Level 2: Workspace reads auto-approve; writes, exec, net require confirmation.
    ReadFree = 2,
    /// Level 3: Workspace all auto-approve; cross-workspace writes require one-time folder trust.
    WorkspaceFree = 3,
    /// Level 4: No permission checks (current default behavior).
    Unrestricted = 4,
}

impl PermissionLevel {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::MaxLockdown,
            2 => Self::ReadFree,
            3 => Self::WorkspaceFree,
            _ => Self::Unrestricted,
        }
    }

    pub fn to_u8(self) -> u8 { self as u8 }

    pub fn label(self) -> &'static str {
        match self {
            Self::MaxLockdown => "Level 1 — Maximum Lockdown",
            Self::ReadFree => "Level 2 — Read Free",
            Self::WorkspaceFree => "Level 3 — Workspace Free",
            Self::Unrestricted => "Level 4 — Unrestricted",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::MaxLockdown => "All operations require confirmation. No automatic trust.",
            Self::ReadFree => "Reads auto-approve. Writes, execution, and network require confirmation.",
            Self::WorkspaceFree => "Auto-approve within workspace. Cross-workspace writes are trusted once per folder.",
            Self::Unrestricted => "No permission checks. All tools execute immediately.",
        }
    }
}

// ──────────────────────────────────────
// Path helpers
// ──────────────────────────────────────

/// Extract file/directory paths from tool arguments that the tool will read or write.
pub fn extract_target_paths(tool_name: &str, args: &serde_json::Value) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Direct path argument
    if let Some(p) = args.get("path").and_then(|v| v.as_str()) {
        paths.push(PathBuf::from(p));
    }
    // Multiple paths
    if let Some(arr) = args.get("paths").and_then(|v| v.as_array()) {
        for v in arr {
            if let Some(s) = v.as_str() {
                paths.push(PathBuf::from(s));
            }
        }
    }
    // source / dest pairs (copy, move)
    if let Some(s) = args.get("source").and_then(|v| v.as_str()) {
        paths.push(PathBuf::from(s));
    }
    if let Some(d) = args.get("dest").and_then(|v| v.as_str()) {
        paths.push(PathBuf::from(d));
    }
    // exec_run: extract cwd
    if tool_name == "exec_run" {
        if let Some(cwd) = args.get("cwd").and_then(|v| v.as_str()) {
            paths.push(PathBuf::from(cwd));
        }
    }

    // Canonicalize where possible for reliable boundary checks
    paths.into_iter()
        .filter_map(|p| {
            if p.is_absolute() {
                Some(std::fs::canonicalize(&p).ok().unwrap_or(p))
            } else {
                // Relative paths: resolve against current directory
                Some(std::env::current_dir().ok()
                    .map(|cwd| cwd.join(&p))
                    .and_then(|abs| std::fs::canonicalize(&abs).ok().or(Some(abs)))
                    .unwrap_or(p))
            }
        })
        .collect()
}

/// Check if ALL target paths are inside the workspace root.
fn all_within_workspace(paths: &[PathBuf], workspace: &Path) -> bool {
    if paths.is_empty() { return true; } // tools without paths (e.g. memory_read) are considered safe
    paths.iter().all(|p| p.starts_with(workspace))
}

/// Find the first path (if any) that is outside the workspace.
fn first_outside_workspace<'a>(paths: &'a [PathBuf], workspace: &Path) -> Option<&'a PathBuf> {
    paths.iter().find(|p| !p.starts_with(workspace))
}

// ──────────────────────────────────────
// Permission decision
// ──────────────────────────────────────

/// Result of `needs_permission()`: either auto-approve or request confirmation.
#[derive(Debug)]
pub enum PermissionDecision {
    /// No confirmation needed — execute immediately.
    AutoApprove,
    /// Confirmation required. Contains the reason and target paths for the dialog.
    AskUser {
        /// Human-readable reason for the dialog (e.g. "Write to external path").
        reason: String,
        /// Paths to display in the dialog.
        paths: Vec<PathBuf>,
        /// Whether the tool is Read/Write/Exec/Net.
        category: ToolCategory,
    },
}

/// Determine whether a tool call requires user permission.
///
/// - `level`: current permission level
/// - `tool_name`: registered tool name
/// - `args`: tool arguments (JSON)
/// - `workspace_root`: workspace root directory (used for boundary checks)
/// - `trusted_dirs`: set of previously trusted directories
pub fn needs_permission(
    level: PermissionLevel,
    tool_name: &str,
    args: &serde_json::Value,
    workspace_root: &Path,
    trusted_dirs: &HashSet<PathBuf>,
) -> PermissionDecision {
    // Level 4: everything auto-approved
    if level == PermissionLevel::Unrestricted {
        return PermissionDecision::AutoApprove;
    }

    let category = categorize_tool(tool_name);
    let paths = extract_target_paths(tool_name, args);

    // Level 1: everything requires confirmation
    if level == PermissionLevel::MaxLockdown {
        return PermissionDecision::AskUser {
            reason: format!("Level 1: '{}' requires confirmation.", tool_name),
            paths,
            category,
        };
    }

    // Level 2+: Reads auto-approve
    if category == ToolCategory::Read {
        return PermissionDecision::AutoApprove;
    }

    // Level 3+: Within-workspace auto-approve; check boundary for Write/Exec/Net
    if level >= PermissionLevel::WorkspaceFree {
        // If no paths or all paths within workspace, auto-approve
        if all_within_workspace(&paths, workspace_root) {
            return PermissionDecision::AutoApprove;
        }

        // Cross-workspace: check trusted folders
        if let Some(outside) = first_outside_workspace(&paths, workspace_root) {
            let dir: &Path = outside.parent().unwrap_or(outside);
            if trusted_dirs.iter().any(|d| d == dir) {
                return PermissionDecision::AutoApprove;
            }
        }
    }

    // Otherwise: ask user
    let reason = if level == PermissionLevel::ReadFree {
        format!("Level 2: '{}' (write/exec/net) requires confirmation.", tool_name)
    } else {
        format!("Level 3: '{}' accesses a path outside the workspace.", tool_name)
    };

    PermissionDecision::AskUser { reason, paths, category }
}

// ──────────────────────────────────────
// Trusted folder set
// ──────────────────────────────────────

/// Persistent set of trusted directories for cross-workspace access.
/// Stored as `{sessions_dir}/{seed}/trusted_folders.json`.
pub struct TrustedFolderSet {
    seed: String,
    dirs: HashSet<PathBuf>,
}

impl TrustedFolderSet {
    /// Load the trusted folders file for a session, or create an empty set.
    pub fn load(seed: &str) -> Self {
        let path = trusted_folders_path(seed);
        let dirs = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
                .map(|v| v.into_iter().map(PathBuf::from).collect())
                .unwrap_or_default()
        } else {
            HashSet::new()
        };
        Self { seed: seed.to_string(), dirs }
    }

    /// Add a directory to the trusted set and persist.
    pub fn trust(&mut self, dir: &Path) {
        self.dirs.insert(dir.to_path_buf());
        self.save();
    }

    /// Check if a directory is trusted.
    pub fn contains(&self, dir: &Path) -> bool {
        self.dirs.contains(dir)
    }

    /// Expose the underlying set for permission checks.
    pub fn set(&self) -> &HashSet<PathBuf> {
        &self.dirs
    }

    fn save(&self) {
        let path = trusted_folders_path(&self.seed);
        let dir = path.parent().unwrap();
        let _ = std::fs::create_dir_all(dir);
        let list: Vec<String> = self.dirs.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let _ = std::fs::write(&path, serde_json::to_string(&list).unwrap_or_default());
    }
}

fn trusted_folders_path(seed: &str) -> PathBuf {
    crate::workspace::deepx_dir().join("sessions").join(seed).join("trusted_folders.json")
}
