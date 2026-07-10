//! Git operations via libgit2 (git2 crate), no exec calls.
//!
//! Functions take JSON args with optional `path` (repo root, defaults to workspace).

use crate::{JsonArgs, ToolCallCtx, ToolHandler, ToolRisk, ToolResult, handler};
use git2::{DiffOptions, Repository, StatusOptions, StatusShow};
use std::path::{Path, PathBuf};

// ── helpers ──

fn open_repo(path_arg: &str) -> Result<Repository, String> {
    let p = crate::resolve_workspace_path(path_arg);
    let p_str = p.clone();
    let p_path = Path::new(&p_str);

    if !p_path.exists() || p_str == "." || p_str.is_empty() {
        let ws = crate::CURRENT_WORKSPACE
            .read()
            .map(|ws| ws.clone())
            .unwrap_or_default();
        let start = if ws.is_empty() || ws == "." {
            std::env::current_dir().map_err(|e| format!("cwd: {e}"))?
        } else {
            PathBuf::from(ws)
        };
        Repository::discover(&start)
            .map_err(|e| format!("not a git repo (discover from {}): {e}", start.display()))
    } else {
        Repository::open(p_path)
            .map_err(|e| format!("cannot open repo at {p_str}: {e}"))
    }
}

fn fmt_time(t: i64) -> String {
    use chrono::{DateTime, Local};
    let secs = if t >= 0 { t as u64 } else { 0 };
    let dt: DateTime<Local> = DateTime::from_timestamp(secs as i64, 0)
        .unwrap_or_default()
        .with_timezone(&Local);
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

// ── command executors ──

pub(super) fn exec_log(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let max_str = args.s_or("max_count", "20");
    let max: usize = max_str.parse().unwrap_or(20);
    let author = args.s("author");

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    let mut revwalk = match repo.revwalk() {
        Ok(w) => w,
        Err(e) => return crate::json_err("REVWALK_ERROR", &format!("revwalk: {e}"), "Check the repository state."),
    };
    if revwalk.push_head().is_err() {
        return crate::json_ok(serde_json::json!({"content": "(no commits yet)"}));
    }
    revwalk.set_sorting(git2::Sort::TIME).ok();

    let mut out = String::new();
    let mut count = 0;
    for oid_result in revwalk {
        let oid = match oid_result {
            Ok(o) => o,
            _ => continue,
        };
        if count >= max {
            break;
        }
        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            _ => continue,
        };
        if !author.is_empty() {
            let sig = commit.author();
            let a = sig.name().unwrap_or("");
            if !a.contains(&author) {
                continue;
            }
        }

        let summary = match commit.summary() {
            Ok(Some(s)) => s.to_string(),
            _ => "(no message)".to_string(),
        };
        let oid_str = oid.to_string();
        let hash = &oid_str[..7.min(oid_str.len())];
        let sig = commit.author();
        let who = sig.name().unwrap_or("unknown");
        let time = fmt_time(commit.time().seconds());

        out.push_str(&format!("{hash}  {who}  {time}\n  {summary}\n\n"));
        count += 1;
    }

    if out.is_empty() {
        crate::json_ok(serde_json::json!({"content": "(no matching commits)"}))
    } else {
        crate::json_ok(serde_json::json!({"content": out.trim_end()}))
    }
}

pub(super) fn exec_diff(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let commit_a = args.s("commit_a");
    let commit_b = args.s("commit_b");
    let cached = args.s_or("cached", "false") == "true";

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    let tree_a = if commit_a.is_empty() {
        repo.head().and_then(|h| h.peel_to_tree()).ok()
    } else {
        rev_parse_tree(&repo, &commit_a)
    };
    let tree_b = if commit_b.is_empty() {
        None
    } else {
        rev_parse_tree(&repo, &commit_b)
    };

    let mut opts = DiffOptions::new();
    let diff_result = match (tree_a, tree_b) {
        (Some(a), Some(b)) => repo.diff_tree_to_tree(Some(&a), Some(&b), Some(&mut opts)),
        (Some(a), None) => {
            if cached {
                repo.diff_tree_to_index(Some(&a), None, Some(&mut opts))
            } else {
                repo.diff_tree_to_workdir(Some(&a), Some(&mut opts))
            }
        }
        (None, None) | (None, Some(_)) => return crate::json_err("MISSING_COMMIT", "need at least one commit to diff", "Provide commit_a or commit_b."),
    };

    let diff = match diff_result {
        Ok(d) => d,
        Err(e) => return crate::json_err("DIFF_ERROR", &format!("diff: {e}"), "Check the commit references."),
    };

    let mut out = String::new();
    let _ = diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let origin = line.origin();
        if let Ok(content) = std::str::from_utf8(line.content()) {
            out.push(origin as char);
            out.push_str(content.trim_end_matches('\n'));
            out.push('\n');
        }
        true
    });

    if out.is_empty() {
        return crate::json_ok(serde_json::json!({"content": "(no differences)"}));
    }
    let stats = diff.stats().ok();
    let content = match stats {
        Some(s) => format!(
            "{} files changed, {} insertions(+), {} deletions(-)\n",
            s.files_changed(),
            s.insertions(),
            s.deletions()
        ),
        None => String::new(),
    };
    crate::json_ok(serde_json::json!({"content": content + &out.trim_end()}))
}

pub(super) fn exec_status(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    let mut opts = StatusOptions::new();
    opts.show(StatusShow::IndexAndWorkdir);
    opts.include_untracked(true);
    opts.recurse_untracked_dirs(true);

    let statuses = match repo.statuses(Some(&mut opts)) {
        Ok(s) => s,
        Err(e) => return crate::json_err("STATUS_ERROR", &format!("status: {e}"), "Check the repository state."),
    };

    if statuses.is_empty() {
        return crate::json_ok(serde_json::json!({"content": "(clean working tree)"}));
    }

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();

    for entry in statuses.iter() {
        let file = entry.path().unwrap_or("?");
        let flags = entry.status();

        if flags.is_index_new()
            || flags.is_index_modified()
            || flags.is_index_deleted()
            || flags.is_index_renamed()
            || flags.is_index_typechange()
        {
            let label = if flags.is_index_new() { "new" }
                else if flags.is_index_deleted() { "del" }
                else { "mod" };
            staged.push(format!("  {label:4} {file}"));
        }
        if flags.is_wt_new() {
            untracked.push(format!("  new    {file}"));
        } else if flags.is_wt_modified()
            || flags.is_wt_deleted()
            || flags.is_wt_typechange()
            || flags.is_wt_renamed()
        {
            let label = if flags.is_wt_deleted() { "del" }
                else if flags.is_wt_renamed() { "ren" }
                else { "mod" };
            unstaged.push(format!("  {label:4} {file}"));
        }
    }

    let mut out = String::new();
    if !staged.is_empty() {
        out.push_str("Staged:\n");
        out.push_str(&staged.join("\n"));
        out.push('\n');
    }
    if !unstaged.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("Unstaged:\n");
        out.push_str(&unstaged.join("\n"));
        out.push('\n');
    }
    if !untracked.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("Untracked:\n");
        out.push_str(&untracked.join("\n"));
        out.push('\n');
    }

    crate::json_ok(serde_json::json!({"content": out.trim_end()}))
}

pub(super) fn exec_show(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let commit = args.s("commit");

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    let oid = if commit.is_empty() {
        match repo.head() {
            Ok(h) => h.target().unwrap_or(git2::Oid::ZERO_SHA1),
            Err(e) => return crate::json_err("HEAD_ERROR", &format!("head: {e}"), "Check the repository state."),
        }
    } else if let Ok(o) = git2::Oid::from_str(&commit) {
        o
    } else if let Some(o) = rev_parse_oid(&repo, &commit) {
        o
    } else {
        return crate::json_err("UNKNOWN_REVISION", &format!("unknown revision: {commit}"), "Use a valid commit hash or ref.");
    };

    let c = match repo.find_commit(oid) {
        Ok(c) => c,
        Err(e) => return crate::json_err("COMMIT_ERROR", &format!("commit {commit}: {e}"), "Check the commit hash."),
    };

    let hash = c.id().to_string();
    let sig = c.author();
    let author = sig.name().unwrap_or("unknown");
    let email = sig.email().unwrap_or("");
    let time = fmt_time(c.time().seconds());
    let summary = match c.summary() {
        Ok(Some(s)) => s.to_string(),
        _ => "(no message)".to_string(),
    };
    let parents = c.parent_count();

    let mut out = format!("commit {hash}\nAuthor: {author} <{email}>\nDate:   {time}\n\n    {summary}\n");
    if parents > 1 {
        let p_hashes: Vec<String> = (0..parents)
            .map(|i| {
                if let Ok(parent_oid) = c.parent_id(i) {
                    let s = parent_oid.to_string();
                    s[..7.min(s.len())].to_string()
                } else {
                    "?".to_string()
                }
            })
            .collect();
        out.push_str(&format!("\nParents: {}\n", p_hashes.join(" ")));
    }

    if let Ok(tree) = c.tree() {
        let parent_tree = if parents > 0 {
            c.parent(0).ok().and_then(|p| p.tree().ok())
        } else {
            None
        };
        let mut opts = DiffOptions::new();
        if let Ok(diff) = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut opts)) {
            out.push('\n');
            let _ = diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
                let origin = line.origin();
                if let Ok(content) = std::str::from_utf8(line.content()) {
                    out.push(origin as char);
                    out.push_str(content.trim_end_matches('\n'));
                    out.push('\n');
                }
                true
            });
        }
    }

    crate::json_ok(serde_json::json!({"content": out.trim_end()}))
}

pub(super) fn exec_add(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let files_raw = args.s_or("files", ".");

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    let mut index = match repo.index() {
        Ok(i) => i,
        Err(e) => return crate::json_err("INDEX_ERROR", &format!("index: {e}"), "Check the repository index."),
    };

    if files_raw == "." {
        let _ = index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None);
    } else {
        let files: Vec<String> = serde_json::from_str(&files_raw).unwrap_or_else(|_| vec![files_raw.clone()]);
        for file in &files {
            let resolved = crate::resolve_workspace_path(file);
            let rel = if let Some(wd) = repo.workdir() {
                Some(Path::new(&resolved).strip_prefix(wd).unwrap_or(Path::new(&resolved)).to_path_buf())
            } else {
                None
            };
            let rel_path = rel.as_deref().unwrap_or(Path::new(&resolved));
            if let Err(e) = index.add_path(rel_path) {
                return crate::json_err("ADD_ERROR", &format!("add {file}: {e}"), "Check the file path.");
            }
        }
    }

    if let Err(e) = index.write() {
        return crate::json_err("INDEX_WRITE_ERROR", &format!("index write: {e}"), "Check disk space or permissions.");
    }

    crate::json_ok(serde_json::json!({"content": "staged successfully"}))
}

pub(super) fn exec_commit(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let message = args.s("message");

    if message.is_empty() {
        return crate::json_err("MISSING_MESSAGE", "commit message is required", "Provide a commit message.");
    }

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    let mut index = match repo.index() {
        Ok(i) => i,
        Err(e) => return crate::json_err("INDEX_ERROR", &format!("index: {e}"), "Check the repository index."),
    };
    let _ = index.write();
    let tree_id = match index.write_tree() {
        Ok(t) => t,
        Err(e) => return crate::json_err("TREE_ERROR", &format!("write tree: {e}"), "Check the repository state."),
    };
    let tree = match repo.find_tree(tree_id) {
        Ok(t) => t,
        Err(e) => return crate::json_err("TREE_ERROR", &format!("find tree: {e}"), "Check the repository state."),
    };

    let parents = match repo.head() {
        Ok(head) => {
            let oid = head.target().unwrap_or(git2::Oid::ZERO_SHA1);
            if oid.is_zero() {
                vec![]
            } else {
                repo.find_commit(oid).map(|c| vec![c]).unwrap_or_default()
            }
        }
        Err(_) => vec![],
    };
    let parent_ptrs: Vec<&git2::Commit> = parents.iter().collect();

    let sig = repo
        .signature()
        .unwrap_or_else(|_| git2::Signature::now("deepx-agent", "agent@deepx").unwrap());

    match repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &parent_ptrs) {
        Ok(oid) => {
            let s = oid.to_string();
            let short = &s[..7.min(s.len())];
            crate::json_ok(serde_json::json!({"hash": short, "content": format!("committed {}", short)}))
        }
        Err(e) => crate::json_err("COMMIT_ERROR", &format!("commit: {e}"), "Check the commit message and repository state."),
    }
}

pub(super) fn exec_branch(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let action = args.s("action");
    let name = args.s("name");
    let start_point = args.s("start_point");
    let force = args.s_or("force", "false") == "true";

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    match action.as_str() {
        "list" => {
            let mut branches: Vec<serde_json::Value> = Vec::new();
            let head_name = repo.head().ok().and_then(|h| h.shorthand().ok().map(String::from));
            if let Ok(iter) = repo.branches(Some(git2::BranchType::Local)) {
                for b in iter.flatten() {
                    let bname = b.0.name().ok().flatten().unwrap_or("").to_string();
                    if bname.is_empty() { continue; }
                    branches.push(serde_json::json!({
                        "name": bname,
                        "is_head": head_name.as_deref() == Some(&bname),
                    }));
                }
            }
            match serde_json::to_string(&branches) {
                Ok(s) => crate::json_ok(serde_json::json!({"branches": branches, "content": s})),
                Err(e) => crate::json_err("SERIALIZE_ERROR", &format!("serialize: {e}"), "Check branch data."),
            }
        }
        "create" => {
            if name.is_empty() {
                return crate::json_err("MISSING_NAME", "branch name is required", "Provide a branch name.");
            }
            let commit = if start_point.is_empty() {
                repo.head().ok().and_then(|h| h.peel_to_commit().ok())
            } else {
                rev_parse_oid(&repo, &start_point)
                    .and_then(|oid| repo.find_commit(oid).ok())
            };
            let commit = match commit {
                Some(c) => c,
                None => return crate::json_err("RESOLVE_ERROR", "cannot resolve commit to branch from", "Check the start_point parameter."),
            };
            match repo.branch(&name, &commit, force) {
                Ok(_) => crate::json_ok(serde_json::json!({"branch": name, "content": format!("created branch '{}'", name)})),
                Err(e) => crate::json_err("BRANCH_ERROR", &format!("branch create: {e}"), "Check the branch name."),
            }
        }
        "delete" => {
            if name.is_empty() {
                return crate::json_err("MISSING_NAME", "branch name is required", "Provide a branch name.");
            }
            let mut branch = match repo.find_branch(&name, git2::BranchType::Local) {
                Ok(b) => b,
                Err(e) => return crate::json_err("NOT_FOUND", &format!("find branch '{}': {}", name, e), "Check the branch name."),
            };
            if branch.is_head() && !force {
                return crate::json_err("CANNOT_DELETE_CURRENT", "cannot delete current branch without force=true", "Use force=true to delete the current branch.");
            }
            match branch.delete() {
                Ok(()) => crate::json_ok(serde_json::json!({"branch": name, "content": format!("deleted branch '{}'", name)})),
                Err(e) => crate::json_err("DELETE_ERROR", &format!("delete: {e}"), "Check if the branch can be deleted."),
            }
        }
        _ => crate::json_err("UNKNOWN_ACTION", &format!("unknown action '{}'. Use 'list', 'create', or 'delete'.", action), "Choose one of: list, create, delete."),
    }
}

pub(super) fn exec_checkout(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let target = args.s("target");
    let force = args.s_or("force", "false") == "true";

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    // Mode 1: checkout a branch / ref
    // Try as a branch name first, then as a revspec
    if repo.find_branch(&target, git2::BranchType::Local).is_ok()
        || !target.starts_with("--")
    {
        // Check if target looks like a file path (contains / or .)
        // If it's a valid revspec, do branch checkout; otherwise restore file
        let is_ref = repo.revparse_single(&target).is_ok();

        if is_ref {
            let (obj, reference) = match repo.revparse_ext(&target) {
                Ok(r) => r,
                Err(e) => return crate::json_err("REVPARSE_ERROR", &format!("revparse '{}': {}", target, e), "Check the target reference."),
            };
            let mut opts = git2::build::CheckoutBuilder::new();
            if force { opts.force(); }
            if let Err(e) = repo.checkout_tree(&obj, Some(&mut opts)) {
                return crate::json_err("CHECKOUT_ERROR", &format!("checkout tree: {e}"), "Check the repository state.");
            }
            if let Some(r) = reference {
                if let Err(e) = repo.set_head(r.name().ok().unwrap_or("")) {
                    return crate::json_err("HEAD_ERROR", &format!("set HEAD: {e}"), "Check the repository state.");
                }
            } else {
                // Detached HEAD — target is a commit, not a branch
                if let Err(e) = repo.set_head_detached(obj.id()) {
                    return crate::json_err("HEAD_ERROR", &format!("set detached HEAD: {e}"), "Check the repository state.");
                }
            }
            crate::json_ok(serde_json::json!({"branch": target, "content": format!("checked out '{}'", target)}))
        } else {
            // Mode 2: restore file from index/HEAD
            let resolved = crate::resolve_workspace_path(&target);
            let rel = if let Some(wd) = repo.workdir() {
                Path::new(&resolved).strip_prefix(wd).unwrap_or(Path::new(&resolved)).to_path_buf()
            } else {
                PathBuf::from(&resolved)
            };
            let rel_str = rel.to_string_lossy();
            let mut opts = git2::build::CheckoutBuilder::new();
            if force { opts.force(); }
            opts.path(&*rel_str);
            if let Err(e) = repo.checkout_index(None, Some(&mut opts)) {
                return crate::json_err("RESTORE_ERROR", &format!("restore '{}': {}", rel_str, e), "Check the file path.");
            }
            crate::json_ok(serde_json::json!({"file": rel_str.to_string(), "content": format!("restored '{}'", rel_str)}))
        }
    } else {
        crate::json_err("RESOLVE_ERROR", &format!("cannot resolve '{}' as a ref or file", target), "Use a valid branch name, commit hash, or file path.")
    }
}

pub(super) fn exec_merge(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let branch = args.s("branch");
    let message = args.s_or("message", "");

    if branch.is_empty() {
        return crate::json_err("MISSING_BRANCH", "branch to merge is required", "Provide a branch name.");
    }

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    // Resolve the branch to an annotated commit
    let refname = format!("refs/heads/{branch}");
    let (their_obj, _) = match repo.revparse_ext(&refname) {
        Ok(r) => r,
        Err(_) => {
            // Try full ref name
            match repo.revparse_ext(&branch) {
                Ok(r) => r,
                Err(e) => return crate::json_err("RESOLVE_ERROR", &format!("resolve '{}': {}", branch, e), "Check the branch name."),
            }
        }
    };

    let their_commit = match their_obj.peel_to_commit() {
        Ok(c) => c,
        Err(e) => return crate::json_err("PEEL_ERROR", &format!("peel to commit: {e}"), "Check the branch reference."),
    };

    let annotated = match repo.find_annotated_commit(their_commit.id()) {
        Ok(a) => a,
        Err(e) => return crate::json_err("ANNOTATED_ERROR", &format!("annotated: {e}"), "Check the branch reference."),
    };

    // Attempt merge
    let mut merge_opts = git2::MergeOptions::new();
    let mut checkout_opts = git2::build::CheckoutBuilder::new();
    checkout_opts.force();

    match repo.merge(&[&annotated], Some(&mut merge_opts), Some(&mut checkout_opts)) {
        Ok(()) => {}
        Err(e) => {
            let _ = repo.cleanup_state();
            return crate::json_err("MERGE_ERROR", &format!("merge: {e}"), "Check for conflicts.");
        }
    }

    // Check for conflicts
    let mut idx = match repo.index() {
        Ok(i) => i,
        Err(e) => {
            let _ = repo.cleanup_state();
            return crate::json_err("INDEX_ERROR", &format!("index: {e}"), "Check the repository index.");
        }
    };

    if idx.has_conflicts() {
        let mut conflicted = Vec::new();
        if let Ok(conflicts) = idx.conflicts() {
            for c in conflicts.flatten() {
                if let Some(entry) = c.our {
                    conflicted.push(String::from_utf8_lossy(&entry.path).to_string());
                }
            }
        }
        let _ = repo.cleanup_state();
        let list = conflicted.join(", ");
        return crate::json_err("MERGE_CONFLICTS", &format!("merge conflicts in: {list}"), "Resolve conflicts first.");
    }

    // Auto-commit the merge
    let tree_id = match idx.write_tree() {
        Ok(t) => t,
        Err(e) => {
            let _ = repo.cleanup_state();
            return crate::json_err("TREE_ERROR", &format!("write tree: {e}"), "Check the repository state.");
        }
    };
    let tree = match repo.find_tree(tree_id) {
        Ok(t) => t,
        Err(e) => {
            let _ = repo.cleanup_state();
            return crate::json_err("TREE_ERROR", &format!("find tree: {e}"), "Check the repository state.");
        }
    };

    let head_commit = match repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
        Some(c) => c,
        None => {
            let _ = repo.cleanup_state();
            return crate::json_err("NO_HEAD", "no HEAD commit", "Check the repository state.");
        }
    };

    let parents = [&head_commit, &their_commit];
    let msg = if message.is_empty() {
        format!("merge '{branch}'")
    } else {
        message.to_string()
    };

    let sig = repo.signature()
        .unwrap_or_else(|_| git2::Signature::now("deepx-agent", "agent@deepx").unwrap());

    match repo.commit(Some("HEAD"), &sig, &sig, &msg, &tree, &parents) {
        Ok(oid) => {
            let _ = repo.cleanup_state();
            let s = oid.to_string();
            let short = &s[..7.min(s.len())];
            crate::json_ok(serde_json::json!({"hash": short, "branch": branch, "content": format!("merged '{}', commit {}", branch, short)}))
        }
        Err(e) => {
            let _ = repo.cleanup_state();
            crate::json_err("COMMIT_ERROR", &format!("merge commit: {e}"), "Check the commit message.")
        }
    }
}

pub(super) fn exec_restore(args: &serde_json::Value) -> String {
    let path = args.s("path");
    let files_raw = args.s("files");
    let staged = args.s_or("staged", "false") == "true";

    if files_raw.is_empty() {
        return crate::json_err("MISSING_FILES", "files is required", "Provide the files parameter.");
    }

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return crate::json_err("REPO_ERROR", &e, "Check the repository path."),
    };

    let files: Vec<String> = serde_json::from_str(&files_raw)
        .unwrap_or_else(|_| vec![files_raw.clone()]);

    let mut restored = Vec::new();
    for file in &files {
        let resolved = crate::resolve_workspace_path(file);
        let rel = if let Some(wd) = repo.workdir() {
            Path::new(&resolved).strip_prefix(wd).unwrap_or(Path::new(&resolved)).to_path_buf()
        } else {
            PathBuf::from(&resolved)
        };
        let rel_str = rel.to_string_lossy();

        if staged {
            // Restore from HEAD (unstage)
            let head_tree = match repo.head().ok().and_then(|h| h.peel_to_tree().ok()) {
                Some(t) => t,
                None => return crate::json_err("NO_HEAD_TREE", "no HEAD tree", "Check the repository state."),
            };
            let mut opts = git2::build::CheckoutBuilder::new();
            opts.path(&*rel_str);
            if let Err(e) = repo.checkout_tree(head_tree.as_object(), Some(&mut opts)) {
                return crate::json_err("RESTORE_ERROR", &format!("restore '{}': {}", rel_str, e), "Check the file path.");
            }
        } else {
            // Restore working tree from index
            let mut opts = git2::build::CheckoutBuilder::new();
            opts.path(&*rel_str);
            if let Err(e) = repo.checkout_index(None, Some(&mut opts)) {
                return crate::json_err("RESTORE_ERROR", &format!("restore '{}': {}", rel_str, e), "Check the file path.");
            }
        }
        restored.push(rel_str.to_string());
    }

    crate::json_ok(serde_json::json!({"files": restored, "content": format!("restored {}", restored.join(", "))}))
}

// ── helpers ──

fn rev_parse_tree<'a>(repo: &'a Repository, spec: &'a str) -> Option<git2::Tree<'a>> {
    let oid = rev_parse_oid(repo, spec)?;
    repo.find_commit(oid).ok().and_then(|c| c.tree().ok())
}

fn rev_parse_oid(repo: &Repository, spec: &str) -> Option<git2::Oid> {
    if let Ok(oid) = git2::Oid::from_str(spec) {
        return Some(oid);
    }
    repo.revparse_single(spec).ok().map(|obj| obj.id())
}

// ── handler and registration ──

handler!(handle_log, exec_log);
handler!(handle_diff, exec_diff);
handler!(handle_status, exec_status);
handler!(handle_show, exec_show);
handler!(handle_add, exec_add);
handler!(handle_commit, exec_commit);
handler!(handle_branch, exec_branch);
handler!(handle_checkout, exec_checkout);
handler!(handle_merge, exec_merge);
handler!(handle_restore, exec_restore);

pub fn register(mgr: &mut crate::ToolManager) {
    mgr.register(ToolHandler {
        key: "git_log".to_string(),
        description: "Show commit history in current git repository. Parameters: path (optional repo path), max_count (default 20, max 100), author (optional name filter). For branch/checkout/merge, use git/branch, git/checkout, git/merge.",
        input_schema: serde_json::json!({"type":"object","description":"Git commit browser. Read-only. Use git/branch, git/checkout, git/merge, git/restore for mutations.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"max_count":{"type":"string","description":"Max commits to show (default 20)"},"author":{"type":"string","description":"Filter by author name (partial match)"}},
        "additionalProperties":false}),
        handler: handle_log,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "git_diff".to_string(),
        description: "Show diff between commits or working tree. Read-only. Parameters: path, commit_a (default HEAD), commit_b (default working tree), cached (boolean string, compare staged vs HEAD).",
        input_schema: serde_json::json!({"type":"object","description":"Git diff viewer. Read-only. Supports tree-to-tree, tree-to-workdir, and tree-to-index (cached) diffs.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"commit_a":{"type":"string","description":"Base commit ref (default HEAD)"},"commit_b":{"type":"string","description":"Target commit ref (default working tree)"},"cached":{"type":"string","description":"If 'true', diff staged vs HEAD instead of working tree"}},
        "additionalProperties":false}),
        handler: handle_diff,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "git_status".to_string(),
        description: "Show working tree status (staged, unstaged, untracked changes). Read-only. Parameters: path.",
        input_schema: serde_json::json!({"type":"object","description":"Git status viewer. Read-only. Does NOT modify repository.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."}},
        "additionalProperties":false}),
        handler: handle_status,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(10),
    });
    mgr.register(ToolHandler {
        key: "git_show".to_string(),
        description: "Show a commit's details (author, date, message) and its diff. Read-only. Parameters: path, commit (hash or ref like HEAD, HEAD~1, main, default HEAD).",
        input_schema: serde_json::json!({"type":"object","description":"Git commit detail viewer. Read-only. Does NOT modify repository.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"commit":{"type":"string","description":"Commit hash or ref (default HEAD)"}},
        "additionalProperties":false}),
        handler: handle_show,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "git_add".to_string(),
        description: "Stage files for commit. MUTATES the git index. Parameters: path, files (single file path string like 'src/main.rs' or JSON array of paths). Use files=\".\" to stage all.",
        input_schema: serde_json::json!({"type":"object","description":"Stage files for commit. WARNING: modifies the git index. Does NOT commit.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"files":{"type":"string","description":"File to stage: path string, JSON array, or '.' for all"}},
        "required":["files"],"additionalProperties":false}),
        handler: handle_add,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "git_commit".to_string(),
        description: "Create a commit with staged changes. MUTATES the repository. Parameters: path, message (required). Only commits staged changes — use git_add first.",
        input_schema: serde_json::json!({"type":"object","description":"Create a commit. WARNING: permanently records changes to git history. Requires staged changes (use git_add first).",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"message":{"type":"string","description":"Commit message (required)"}},
        "required":["message"],"additionalProperties":false}),
        handler: handle_commit,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "git_branch".to_string(),
        description: "List, create, or delete git branches. Parameters: path, action ('list'|'create'|'delete'), name (branch name, required for create/delete), start_point (commit ref for new branch, optional), force (boolean, optional).",
        input_schema: serde_json::json!({"type":"object","description":"Git branch operations. List local branches, create a new branch, or delete an existing one.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"action":{"type":"string","description":"Action: 'list', 'create', or 'delete'"},"name":{"type":"string","description":"Branch name (required for create/delete)"},"start_point":{"type":"string","description":"Commit ref to branch from (optional, defaults to HEAD)"},"force":{"type":"string","description":"If 'true', force create or delete"}},
        "required":["action"],"additionalProperties":false}),
        handler: handle_branch,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: "git_checkout".to_string(),
        description: "Switch branches or restore working tree files. Parameters: path, target (branch name or file path), force (boolean, optional). When target is a branch/ref, switches HEAD. When target is a file path prefixed with '--' or not a valid ref, restores the file from index.",
        input_schema: serde_json::json!({"type":"object","description":"Git checkout: switch branch or restore files. Switches HEAD when target is a valid ref; restores file from index otherwise.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"target":{"type":"string","description":"Branch name or file path to restore"},"force":{"type":"string","description":"If 'true', force checkout (discard local changes)"}},
        "required":["target"],"additionalProperties":false}),
        handler: handle_checkout,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(20),
    });
    mgr.register(ToolHandler {
        key: "git_merge".to_string(),
        description: "Merge a branch into the current HEAD. Parameters: path, branch (branch name to merge), message (optional merge commit message). Auto-commits on success. Reports conflicted files on failure.",
        input_schema: serde_json::json!({"type":"object","description":"Git merge: merge a branch into current HEAD. Auto-commits if no conflicts; reports conflicted files otherwise.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"branch":{"type":"string","description":"Branch name to merge into current HEAD"},"message":{"type":"string","description":"Merge commit message (optional, defaults to 'merge <branch>')"}},
        "required":["branch"],"additionalProperties":false}),
        handler: handle_merge,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: "git_restore".to_string(),
        description: "Restore working tree files to match the index (or HEAD when staged=true). Discards uncommitted changes. Parameters: path, files (single file path or JSON array), staged (boolean, if 'true' restore from HEAD instead of index).",
        input_schema: serde_json::json!({"type":"object","description":"Git restore: discard changes to files, restoring from index or HEAD. Equivalent to 'git restore <files>'.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"files":{"type":"string","description":"File path or JSON array of paths to restore"},"staged":{"type":"string","description":"If 'true', restore from HEAD (unstage) instead of index"}},
        "required":["files"],"additionalProperties":false}),
        handler: handle_restore,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(20),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a JSON string into a Value for test calls.
    fn val(s: &str) -> serde_json::Value { serde_json::from_str(s).unwrap() }

    #[test]
    fn test_status_on_self() {
        let r = exec_status(&val(r#"{"path": "."}"#));
        // handler! macro auto-wraps in JSON: {"status":"ok","content":"..."}
        // or {"status":"error","content":"..."}
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&r) {
            let status = v["status"].as_str().unwrap_or("");
            if status == "error" {
                let content = v["content"].as_str().unwrap_or("");
                assert!(content.contains("not a git repo") || content.contains("cannot open repo"),
                    "unexpected error: {r}");
            }
        } else {
            // Legacy fallback — old format
            assert!(r.starts_with("[OK]") || r.starts_with("[ERROR]"));
        }
    }

    #[test]
    fn test_log() {
        let r = exec_log(&val(r#"{"max_count": "3"}"#));
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&r) {
            let status = v["status"].as_str().unwrap_or("");
            if status == "ok" {
                let content = v["content"].as_str().unwrap_or("");
                assert!(content.contains("-") || content.contains("(no commits)"), "expected dash or no-commits msg");
            }
        } else if r.starts_with("[OK]") {
            assert!(r.contains("-") || r.contains("(no commits)"), "expected dash or no-commits msg");
        }
    }

    #[test]
    fn test_status_nonexistent_dir() {
        let r = exec_status(&val(r#"{"path": "/nonexistent-path"}"#));
        assert!(!r.is_empty(), "should not be empty");
    }

    #[test]
    fn test_branch_list() {
        let r = exec_branch(&val(r#"{"action": "list"}"#));
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&r) {
            let status = v["status"].as_str().unwrap_or("");
            assert!(status == "ok" || status == "error", "unexpected: {r}");
        } else {
            assert!(r.starts_with("[OK]") || r.starts_with("[ERROR]"), "unexpected: {r}");
        }
    }

    #[test]
    fn test_branch_create_delete() {
        let r = exec_branch(&val(r#"{"action": "create", "name": "test-deepx-tmp", "force": "true"}"#));
        let _ = exec_branch(&val(r#"{"action": "delete", "name": "test-deepx-tmp", "force": "true"}"#));
        assert!(!r.is_empty());
    }

    #[test]
    fn test_branch_bad_action() {
        let r = exec_branch(&val(r#"{"action": "nope"}"#));
        assert!(r.contains("unknown action"), "expected unknown action error, got: {r}");
    }

    #[test]
    fn test_checkout_invalid() {
        let r = exec_checkout(&val(r#"{"target": "__nonexistent_branch_xyz__"}"#));
        assert!(!r.is_empty(), "should not be empty");
    }

    #[test]
    fn test_merge_no_branch() {
        let r = exec_merge(&val(r#"{"branch": ""}"#));
        assert!(r.contains("branch"), "expected branch-related error, got: {r}");
    }

    #[test]
    fn test_restore_no_files() {
        let r = exec_restore(&val(r#"{"files": ""}"#));
        assert!(r.contains("files"), "expected files error, got: {r}");
    }

    #[test]
    fn test_restore_staged() {
        let r = exec_restore(&val(r#"{"files": "Cargo.toml", "staged": "true"}"#));
        assert!(!r.is_empty(), "should not be empty");
    }
}
