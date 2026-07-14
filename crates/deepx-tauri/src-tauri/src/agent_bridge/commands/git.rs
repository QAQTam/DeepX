//! Git operation commands: diff, branch listing, switch, commit.
//!
//! All git operations delegate to `deepx_tools::git`. This module only
//! resolves the workspace path from the session seed and calls the
//! appropriate utility function.

/// Resolve the workspace directory for a session seed.
fn resolve_workspace(seed: &str) -> Result<String, String> {
    let dir = deepx_types::platform::sessions_dir().join(seed);
    let ws_path = dir.join("workspace.txt");
    let workspace = std::fs::read_to_string(&ws_path)
        .unwrap_or_default()
        .trim()
        .to_string();
    if workspace.is_empty() {
        Err("no workspace".into())
    } else {
        Ok(workspace)
    }
}

/// Get git status/diff for all changed files in the workspace.
#[tauri::command]
pub fn cmd_get_git_diff(seed: String) -> Result<String, String> {
    let workspace = match resolve_workspace(&seed) {
        Ok(w) => w,
        Err(_) => return Ok("[]".into()),
    };
    deepx_tools::git::status_json(&workspace)
}

/// Get the current git branch name for the workspace.
#[tauri::command]
pub fn cmd_get_git_branch(seed: String) -> Result<String, String> {
    let workspace = resolve_workspace(&seed)?;
    deepx_tools::git::current_branch(&workspace)
}

/// List all local branches with current marked.
#[tauri::command]
pub fn cmd_list_branches(seed: String) -> Result<String, String> {
    let workspace = resolve_workspace(&seed)?;
    deepx_tools::git::list_branches(&workspace)
}

/// Switch to a different branch in the workspace.
#[tauri::command]
pub fn cmd_switch_branch(seed: String, branch: String, stash: bool) -> Result<String, String> {
    let workspace = resolve_workspace(&seed)?;
    deepx_tools::git::switch_branch(&workspace, &branch, stash)
}

/// Commit all staged changes in the workspace.
#[tauri::command]
pub fn cmd_git_commit(seed: String, message: String) -> Result<String, String> {
    let workspace = resolve_workspace(&seed)?;
    deepx_tools::git::commit_all(&workspace, &message)
}

/// Get the diff for a single file in the workspace git repo.
#[tauri::command]
pub fn cmd_get_git_file_diff(seed: String, file_path: String) -> Result<String, String> {
    let workspace = resolve_workspace(&seed)?;
    deepx_tools::git::file_diff(&workspace, &file_path)
}
