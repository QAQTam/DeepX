//! Git operation commands: diff, branch listing, switch, commit.


#[tauri::command]
pub fn cmd_get_git_diff(seed: String) -> Result<String, String> {
    let workspace = {
        let dir = deepx_types::platform::sessions_dir().join(&seed);
        let ws_path = dir.join("workspace.txt");
        std::fs::read_to_string(&ws_path)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    if workspace.is_empty() {
        return Ok("[]".into());
    }

    let repo = match git2::Repository::open(&workspace) {
        Ok(r) => r,
        Err(_) => return Ok("[]".into()),
    };

    let mut files: Vec<serde_json::Value> = Vec::new();

    if let Ok(statuses) = repo.statuses(None) {
        for entry in statuses.iter() {
            let path = entry.path().unwrap_or("").to_string();
            let status = entry.status();
            let change = if status.is_index_new() || status.is_wt_new() {
                "added"
            } else if status.is_index_deleted() || status.is_wt_deleted() {
                "deleted"
            } else if status.is_index_modified() || status.is_wt_modified() {
                "modified"
            } else if status.is_index_renamed() || status.is_wt_renamed() {
                "renamed"
            } else {
                continue;
            };

            // Per-file line stats: diff just this file against HEAD.
            let (lines_added, lines_removed) = if matches!(change, "modified" | "added") {
                let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
                let mut opts = git2::DiffOptions::new();
                opts.pathspec(&path);
                head_tree
                    .and_then(|tree| repo.diff_tree_to_workdir(Some(&tree), Some(&mut opts)).ok())
                    .and_then(|d| d.stats().ok())
                    .map(|s| (s.insertions() as u32, s.deletions() as u32))
                    .unwrap_or((0, 0))
            } else {
                (0, 0)
            };

            files.push(serde_json::json!({
                "path": path,
                "change": change,
                "lines_added": lines_added,
                "lines_removed": lines_removed,
            }));
        }
    }
    serde_json::to_string(&files).map_err(|e| format!("serialize: {e}"))
}

/// Get the current git branch name for the workspace.

#[tauri::command]
pub fn cmd_get_git_branch(seed: String) -> Result<String, String> {
    let workspace = {
        let dir = deepx_types::platform::sessions_dir().join(&seed);
        let ws_path = dir.join("workspace.txt");
        std::fs::read_to_string(&ws_path)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    if workspace.is_empty() {
        return Ok("".into());
    }
    let repo = git2::Repository::open(&workspace).map_err(|_| "not a repo")?;
    let head = repo.head().map_err(|_| "no HEAD")?;
    if head.is_branch() {
        Ok(head.shorthand().unwrap_or("HEAD").to_string())
    } else {
        Ok("HEAD".into())
    }
}

/// List all local branches with current marked.

#[tauri::command]
pub fn cmd_list_branches(seed: String) -> Result<String, String> {
    let workspace = {
        let dir = deepx_types::platform::sessions_dir().join(&seed);
        let ws_path = dir.join("workspace.txt");
        std::fs::read_to_string(&ws_path)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    if workspace.is_empty() {
        return Ok("[]".into());
    }
    let repo = git2::Repository::open(&workspace).map_err(|_| "not a repo")?;
    let head_name = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().ok().map(String::from));

    let mut branches: Vec<serde_json::Value> = Vec::new();
    if let Ok(iter) = repo.branches(Some(git2::BranchType::Local)) {
        for b in iter.flatten() {
            let name = b.0.name().ok().flatten().unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            branches.push(serde_json::json!({
                "name": name,
                "current": head_name.as_deref() == Some(&name),
            }));
        }
    }
    serde_json::to_string(&branches).map_err(|e| format!("serialize: {e}"))
}

/// Switch to a different branch in the workspace.
/// If `stash` is true and there are uncommitted changes, stash them before
/// switching and pop them back afterwards.

#[tauri::command]
pub fn cmd_switch_branch(seed: String, branch: String, stash: bool) -> Result<String, String> {
    let workspace = {
        let dir = deepx_types::platform::sessions_dir().join(&seed);
        let ws_path = dir.join("workspace.txt");
        std::fs::read_to_string(&ws_path)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    if workspace.is_empty() {
        return Err("no workspace".into());
    }
    let mut repo = git2::Repository::open(&workspace).map_err(|e| format!("open repo: {e}"))?;

    // Check for dirty worktree before switching
    let has_changes = repo.statuses(None).map(|s| !s.is_empty()).unwrap_or(false);

    // Stash before switching
    let mut stashed = false;
    if stash && has_changes {
        let sig =
            git2::Signature::now("DeepX", "deepx@local").map_err(|e| format!("signature: {e}"))?;
        repo.stash_save(&sig, "deepx-auto-stash", None)
            .map_err(|e| format!("stash: {e}"))?;
        stashed = true;
    }

    {
        let branch_ref = repo
            .find_branch(&branch, git2::BranchType::Local)
            .map_err(|e| format!("find branch '{}': {}", branch, e))?;
        let obj = branch_ref
            .get()
            .peel(git2::ObjectType::Tree)
            .map_err(|e| format!("peel: {e}"))?;

        let mut checkout_opts = git2::build::CheckoutBuilder::new();
        checkout_opts.force();
        repo.checkout_tree(&obj, Some(&mut checkout_opts))
            .map_err(|e| format!("checkout tree: {e}"))?;

        repo.set_head(branch_ref.get().name().ok().unwrap_or(""))
            .map_err(|e| format!("set HEAD: {e}"))?;
    }

    // Pop the stash we saved earlier
    if stashed {
        if let Err(e) = repo.stash_pop(0, None) {
            log::warn!("stash pop failed (likely conflict, stash kept): {e}");
        }
    }

    let new_head = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().ok().map(String::from))
        .unwrap_or_default();
    Ok(new_head)
}

/// Commit all staged changes in the workspace.

#[tauri::command]
pub fn cmd_git_commit(seed: String, message: String) -> Result<String, String> {
    let workspace = {
        let dir = deepx_types::platform::sessions_dir().join(&seed);
        let ws_path = dir.join("workspace.txt");
        std::fs::read_to_string(&ws_path)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    if workspace.is_empty() {
        return Err("no workspace".into());
    }
    let repo = git2::Repository::open(&workspace).map_err(|e| format!("open repo: {e}"))?;

    // Stage all changes
    let mut index = repo.index().map_err(|e| format!("index: {e}"))?;
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .map_err(|e| format!("add_all: {e}"))?;
    index.write().map_err(|e| format!("index write: {e}"))?;

    // Write tree from index
    let tree_oid = index.write_tree().map_err(|e| format!("write_tree: {e}"))?;
    let tree = repo
        .find_tree(tree_oid)
        .map_err(|e| format!("find_tree: {e}"))?;

    // Get HEAD commit as parent
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.iter().collect();

    // Create signature
    let sig =
        git2::Signature::now("DeepX", "deepx@local").map_err(|e| format!("signature: {e}"))?;

    // Commit, updating HEAD
    let oid = repo
        .commit(Some("HEAD"), &sig, &sig, &message, &tree, &parents)
        .map_err(|e| format!("commit: {e}"))?;

    Ok(oid.to_string())
}

/// Get the diff for a single file in the workspace git repo.

#[tauri::command]
pub fn cmd_get_git_file_diff(seed: String, file_path: String) -> Result<String, String> {
    let workspace = {
        let dir = deepx_types::platform::sessions_dir().join(&seed);
        let ws_path = dir.join("workspace.txt");
        std::fs::read_to_string(&ws_path)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    if workspace.is_empty() {
        return Ok("".into());
    }

    let repo = git2::Repository::open(&workspace).map_err(|e| format!("open repo: {e}"))?;
    let head = repo.head().map_err(|e| format!("head: {e}"))?;
    let head_tree = head.peel_to_tree().map_err(|e| format!("tree: {e}"))?;

    let mut diff_opts = git2::DiffOptions::new();
    diff_opts.pathspec(&file_path);

    let diff = repo
        .diff_tree_to_workdir(Some(&head_tree), Some(&mut diff_opts))
        .map_err(|e| format!("diff: {e}"))?;

    let mut patch_text = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let origin = line.origin();
        let content = std::str::from_utf8(line.content()).unwrap_or("");
        patch_text.push(origin);
        patch_text.push_str(content);
        true
    })
    .map_err(|e| format!("print diff: {e}"))?;

    Ok(patch_text)
}
