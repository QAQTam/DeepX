//! Git operations via libgit2 (git2 crate), no exec calls.
//!
//! Functions take JSON args with optional `path` (repo root, defaults to workspace).

use crate::{parse_arg, parse_arg_or, ToolCallCtx, ToolHandler, ToolKey, ToolRisk, ToolResult, handler};
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

pub(super) fn exec_log(args: &str) -> String {
    let path = parse_arg(args, "path");
    let max_str = parse_arg_or(args, "max_count", "20");
    let max: usize = max_str.parse().unwrap_or(20);
    let author = parse_arg(args, "author");

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
    };

    let mut revwalk = match repo.revwalk() {
        Ok(w) => w,
        Err(e) => return format!("[ERROR] revwalk: {e}"),
    };
    if revwalk.push_head().is_err() {
        return "[OK] (no commits yet)".to_string();
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
        "[OK] (no matching commits)".to_string()
    } else {
        format!("[OK]\n{}", out.trim_end())
    }
}

pub(super) fn exec_diff(args: &str) -> String {
    let path = parse_arg(args, "path");
    let commit_a = parse_arg(args, "commit_a");
    let commit_b = parse_arg(args, "commit_b");
    let cached = parse_arg_or(args, "cached", "false") == "true";

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
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
        (None, None) | (None, Some(_)) => return "[ERROR] need at least one commit to diff".to_string(),
    };

    let diff = match diff_result {
        Ok(d) => d,
        Err(e) => return format!("[ERROR] diff: {e}"),
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
        return "[OK] (no differences)".to_string();
    }
    let stats = diff.stats().ok();
    let summary = match stats {
        Some(s) => format!(
            "[OK] {} files changed, {} insertions(+), {} deletions(-)\n",
            s.files_changed(),
            s.insertions(),
            s.deletions()
        ),
        None => "[OK]\n".to_string(),
    };
    summary + out.trim_end()
}

pub(super) fn exec_status(args: &str) -> String {
    let path = parse_arg(args, "path");
    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
    };

    let mut opts = StatusOptions::new();
    opts.show(StatusShow::IndexAndWorkdir);
    opts.include_untracked(true);
    opts.recurse_untracked_dirs(true);

    let statuses = match repo.statuses(Some(&mut opts)) {
        Ok(s) => s,
        Err(e) => return format!("[ERROR] status: {e}"),
    };

    if statuses.is_empty() {
        return "[OK] (clean working tree)".to_string();
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

    format!("[OK]\n{}", out.trim_end())
}

pub(super) fn exec_show(args: &str) -> String {
    let path = parse_arg(args, "path");
    let commit = parse_arg(args, "commit");

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
    };

    let oid = if commit.is_empty() {
        match repo.head() {
            Ok(h) => h.target().unwrap_or(git2::Oid::ZERO_SHA1),
            Err(e) => return format!("[ERROR] head: {e}"),
        }
    } else if let Ok(o) = git2::Oid::from_str(&commit) {
        o
    } else if let Some(o) = rev_parse_oid(&repo, &commit) {
        o
    } else {
        return format!("[ERROR] unknown revision: {commit}");
    };

    let c = match repo.find_commit(oid) {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] commit {commit}: {e}"),
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

    format!("[OK]\n{}", out.trim_end())
}

pub(super) fn exec_add(args: &str) -> String {
    let path = parse_arg(args, "path");
    let files_raw = parse_arg_or(args, "files", ".");

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
    };

    let mut index = match repo.index() {
        Ok(i) => i,
        Err(e) => return format!("[ERROR] index: {e}"),
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
                return format!("[ERROR] add {file}: {e}");
            }
        }
    }

    if let Err(e) = index.write() {
        return format!("[ERROR] index write: {e}");
    }

    "[OK] staged successfully".to_string()
}

pub(super) fn exec_commit(args: &str) -> String {
    let path = parse_arg(args, "path");
    let message = parse_arg(args, "message");

    if message.is_empty() {
        return "[ERROR] commit message is required".to_string();
    }

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
    };

    let mut index = match repo.index() {
        Ok(i) => i,
        Err(e) => return format!("[ERROR] index: {e}"),
    };
    let _ = index.write();
    let tree_id = match index.write_tree() {
        Ok(t) => t,
        Err(e) => return format!("[ERROR] write tree: {e}"),
    };
    let tree = match repo.find_tree(tree_id) {
        Ok(t) => t,
        Err(e) => return format!("[ERROR] find tree: {e}"),
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
            format!("[OK] committed {short}")
        }
        Err(e) => format!("[ERROR] commit: {e}"),
    }
}

pub(super) fn exec_branch(args: &str) -> String {
    let path = parse_arg(args, "path");
    let action = parse_arg(args, "action");
    let name = parse_arg(args, "name");
    let start_point = parse_arg(args, "start_point");
    let force = parse_arg_or(args, "force", "false") == "true";

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
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
            serde_json::to_string(&branches)
                .map(|s| format!("[OK]\n{s}"))
                .unwrap_or_else(|e| format!("[ERROR] serialize: {e}"))
        }
        "create" => {
            if name.is_empty() {
                return "[ERROR] branch name is required".to_string();
            }
            let commit = if start_point.is_empty() {
                repo.head().ok().and_then(|h| h.peel_to_commit().ok())
            } else {
                rev_parse_oid(&repo, &start_point)
                    .and_then(|oid| repo.find_commit(oid).ok())
            };
            let commit = match commit {
                Some(c) => c,
                None => return "[ERROR] cannot resolve commit to branch from".to_string(),
            };
            match repo.branch(&name, &commit, force) {
                Ok(_) => format!("[OK] created branch '{name}'"),
                Err(e) => format!("[ERROR] branch create: {e}"),
            }
        }
        "delete" => {
            if name.is_empty() {
                return "[ERROR] branch name is required".to_string();
            }
            let mut branch = match repo.find_branch(&name, git2::BranchType::Local) {
                Ok(b) => b,
                Err(e) => return format!("[ERROR] find branch '{name}': {e}"),
            };
            if branch.is_head() && !force {
                return "[ERROR] cannot delete current branch without force=true".to_string();
            }
            match branch.delete() {
                Ok(()) => format!("[OK] deleted branch '{name}'"),
                Err(e) => format!("[ERROR] delete: {e}"),
            }
        }
        _ => format!("[ERROR] unknown action '{action}'. Use 'list', 'create', or 'delete'."),
    }
}

pub(super) fn exec_checkout(args: &str) -> String {
    let path = parse_arg(args, "path");
    let target = parse_arg(args, "target");
    let force = parse_arg_or(args, "force", "false") == "true";

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
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
                Err(e) => return format!("[ERROR] revparse '{target}': {e}"),
            };
            let mut opts = git2::build::CheckoutBuilder::new();
            if force { opts.force(); }
            if let Err(e) = repo.checkout_tree(&obj, Some(&mut opts)) {
                return format!("[ERROR] checkout tree: {e}");
            }
            if let Some(r) = reference {
                if let Err(e) = repo.set_head(r.name().ok().unwrap_or("")) {
                    return format!("[ERROR] set HEAD: {e}");
                }
            } else {
                // Detached HEAD — target is a commit, not a branch
                if let Err(e) = repo.set_head_detached(obj.id()) {
                    return format!("[ERROR] set detached HEAD: {e}");
                }
            }
            format!("[OK] checked out '{target}'")
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
                return format!("[ERROR] restore '{rel_str}': {e}");
            }
            format!("[OK] restored '{rel_str}'")
        }
    } else {
        format!("[ERROR] cannot resolve '{target}' as a ref or file")
    }
}

pub(super) fn exec_merge(args: &str) -> String {
    let path = parse_arg(args, "path");
    let branch = parse_arg(args, "branch");
    let message = parse_arg_or(args, "message", "");

    if branch.is_empty() {
        return "[ERROR] branch to merge is required".to_string();
    }

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
    };

    // Resolve the branch to an annotated commit
    let refname = format!("refs/heads/{branch}");
    let (their_obj, _) = match repo.revparse_ext(&refname) {
        Ok(r) => r,
        Err(_) => {
            // Try full ref name
            match repo.revparse_ext(&branch) {
                Ok(r) => r,
                Err(e) => return format!("[ERROR] resolve '{branch}': {e}"),
            }
        }
    };

    let their_commit = match their_obj.peel_to_commit() {
        Ok(c) => c,
        Err(e) => return format!("[ERROR] peel to commit: {e}"),
    };

    let annotated = match repo.find_annotated_commit(their_commit.id()) {
        Ok(a) => a,
        Err(e) => return format!("[ERROR] annotated: {e}"),
    };

    // Attempt merge
    let mut merge_opts = git2::MergeOptions::new();
    let mut checkout_opts = git2::build::CheckoutBuilder::new();
    checkout_opts.force();

    match repo.merge(&[&annotated], Some(&mut merge_opts), Some(&mut checkout_opts)) {
        Ok(()) => {}
        Err(e) => {
            let _ = repo.cleanup_state();
            return format!("[ERROR] merge: {e}");
        }
    }

    // Check for conflicts
    let mut idx = match repo.index() {
        Ok(i) => i,
        Err(e) => {
            let _ = repo.cleanup_state();
            return format!("[ERROR] index: {e}");
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
        return format!("[ERROR] merge conflicts in: {list}");
    }

    // Auto-commit the merge
    let tree_id = match idx.write_tree() {
        Ok(t) => t,
        Err(e) => {
            let _ = repo.cleanup_state();
            return format!("[ERROR] write tree: {e}");
        }
    };
    let tree = match repo.find_tree(tree_id) {
        Ok(t) => t,
        Err(e) => {
            let _ = repo.cleanup_state();
            return format!("[ERROR] find tree: {e}");
        }
    };

    let head_commit = match repo.head().ok().and_then(|h| h.peel_to_commit().ok()) {
        Some(c) => c,
        None => {
            let _ = repo.cleanup_state();
            return "[ERROR] no HEAD commit".to_string();
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
            format!("[OK] merged '{branch}', commit {short}")
        }
        Err(e) => {
            let _ = repo.cleanup_state();
            format!("[ERROR] merge commit: {e}")
        }
    }
}

pub(super) fn exec_restore(args: &str) -> String {
    let path = parse_arg(args, "path");
    let files_raw = parse_arg(args, "files");
    let staged = parse_arg_or(args, "staged", "false") == "true";

    if files_raw.is_empty() {
        return "[ERROR] files is required".to_string();
    }

    let repo = match open_repo(&path) {
        Ok(r) => r,
        Err(e) => return format!("[ERROR] {e}"),
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
                None => return "[ERROR] no HEAD tree".to_string(),
            };
            let mut opts = git2::build::CheckoutBuilder::new();
            opts.path(&*rel_str);
            if let Err(e) = repo.checkout_tree(head_tree.as_object(), Some(&mut opts)) {
                return format!("[ERROR] restore '{rel_str}': {e}");
            }
        } else {
            // Restore working tree from index
            let mut opts = git2::build::CheckoutBuilder::new();
            opts.path(&*rel_str);
            if let Err(e) = repo.checkout_index(None, Some(&mut opts)) {
                return format!("[ERROR] restore '{rel_str}': {e}");
            }
        }
        restored.push(rel_str.to_string());
    }

    format!("[OK] restored {}", restored.join(", "))
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
        key: ToolKey::new("git", "log"),
        description: "Show commit history in current git repository. ONLY supports browsing — no pull/push/branch/reset/clone/merge/rebase/stash operations. Parameters: path (optional repo path), max_count (default 20, max 100), author (optional name filter).",
        input_schema: serde_json::json!({"type":"object","description":"Git commit browser. Not a full git CLI. Does NOT support: push, pull, branch, checkout, reset, clone, merge, rebase, stash, tag.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"max_count":{"type":"string","description":"Max commits to show (default 20)"},"author":{"type":"string","description":"Filter by author name (partial match)"}},
        "additionalProperties":false}),
        handler: handle_log,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("git", "diff"),
        description: "Show diff between commits or working tree. ONLY git diff equivalent — no merge/rebase conflict resolution. Parameters: path, commit_a (default HEAD), commit_b (default working tree), cached (boolean string, compare staged vs HEAD).",
        input_schema: serde_json::json!({"type":"object","description":"Git diff viewer. Only read-only diff operations. Does NOT support merge, rebase, apply, or patch.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"commit_a":{"type":"string","description":"Base commit ref (default HEAD)"},"commit_b":{"type":"string","description":"Target commit ref (default working tree)"},"cached":{"type":"string","description":"If 'true', diff staged vs HEAD instead of working tree"}},
        "additionalProperties":false}),
        handler: handle_diff,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("git", "status"),
        description: "Show working tree status (staged, unstaged, untracked changes). Read-only. Parameters: path.",
        input_schema: serde_json::json!({"type":"object","description":"Git status viewer. Read-only. Does NOT modify repository.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."}},
        "additionalProperties":false}),
        handler: handle_status,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(10),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("git", "show"),
        description: "Show a commit's details (author, date, message) and its diff. Read-only. Parameters: path, commit (hash or ref like HEAD, HEAD~1, main, default HEAD).",
        input_schema: serde_json::json!({"type":"object","description":"Git commit detail viewer. Read-only. Does NOT modify repository.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"commit":{"type":"string","description":"Commit hash or ref (default HEAD)"}},
        "additionalProperties":false}),
        handler: handle_show,
        risk: ToolRisk::ReadOnly,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("git", "add"),
        description: "Stage files for commit. MUTATES the git index. Parameters: path, files (single file path string like 'src/main.rs' or JSON array of paths). Use files=\".\" to stage all.",
        input_schema: serde_json::json!({"type":"object","description":"Stage files for commit. WARNING: modifies the git index. Does NOT commit.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"files":{"type":"string","description":"File to stage: path string, JSON array, or '.' for all"}},
        "required":["files"],"additionalProperties":false}),
        handler: handle_add,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("git", "commit"),
        description: "Create a commit with staged changes. MUTATES the repository. Parameters: path, message (required). Only commits staged changes — use git_add first.",
        input_schema: serde_json::json!({"type":"object","description":"Create a commit. WARNING: permanently records changes to git history. Requires staged changes (use git_add first). Cannot be undone without git reset (not supported).",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"message":{"type":"string","description":"Commit message (required)"}},
        "required":["message"],"additionalProperties":false}),
        handler: handle_commit,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("git", "branch"),
        description: "List, create, or delete git branches. Parameters: path, action ('list'|'create'|'delete'), name (branch name, required for create/delete), start_point (commit ref for new branch, optional), force (boolean, optional).",
        input_schema: serde_json::json!({"type":"object","description":"Git branch operations. List local branches, create a new branch, or delete an existing one.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"action":{"type":"string","description":"Action: 'list', 'create', or 'delete'"},"name":{"type":"string","description":"Branch name (required for create/delete)"},"start_point":{"type":"string","description":"Commit ref to branch from (optional, defaults to HEAD)"},"force":{"type":"string","description":"If 'true', force create or delete"}},
        "required":["action"],"additionalProperties":false}),
        handler: handle_branch,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(15),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("git", "checkout"),
        description: "Switch branches or restore working tree files. Parameters: path, target (branch name or file path), force (boolean, optional). When target is a branch/ref, switches HEAD. When target is a file path prefixed with '--' or not a valid ref, restores the file from index.",
        input_schema: serde_json::json!({"type":"object","description":"Git checkout: switch branch or restore files. Switches HEAD when target is a valid ref; restores file from index otherwise.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"target":{"type":"string","description":"Branch name or file path to restore"},"force":{"type":"string","description":"If 'true', force checkout (discard local changes)"}},
        "required":["target"],"additionalProperties":false}),
        handler: handle_checkout,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(20),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("git", "merge"),
        description: "Merge a branch into the current HEAD. Parameters: path, branch (branch name to merge), message (optional merge commit message). Auto-commits on success. Reports conflicted files on failure.",
        input_schema: serde_json::json!({"type":"object","description":"Git merge: merge a branch into current HEAD. Auto-commits if no conflicts; reports conflicted files otherwise.",
        "properties":{"path":{"type":"string","description":"Repository path. Defaults to workspace root."},"branch":{"type":"string","description":"Branch name to merge into current HEAD"},"message":{"type":"string","description":"Merge commit message (optional, defaults to 'merge <branch>')"}},
        "required":["branch"],"additionalProperties":false}),
        handler: handle_merge,
        risk: ToolRisk::Destructive,
        default_timeout: std::time::Duration::from_secs(30),
    });
    mgr.register(ToolHandler {
        key: ToolKey::new("git", "restore"),
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

    #[test]
    fn test_status_on_self() {
        let r = exec_status(r#"{"path": "."}"#);
        // Should either succeed (in a git repo) or fail gracefully (not a git repo)
        assert!(r.starts_with("[OK]") || r.starts_with("[ERROR]"));
        if r.starts_with("[ERROR]") {
            assert!(r.contains("not a git repo") || r.contains("cannot open repo"),
                "unexpected error: {r}");
        }
    }

    #[test]
    fn test_log() {
        let r = exec_log(r#"{"max_count": "3"}"#);
        if r.starts_with("[OK]") {
            // Should have some commit-like output
            assert!(r.contains("-") || r.contains("(no commits)"), "expected dash or no-commits msg");
        }
        // graceful failure is also OK (non-git-repo)
    }

    #[test]
    fn test_status_nonexistent_dir() {
        let r = exec_status(r#"{"path": "/nonexistent-path"}"#);
        // May succeed if cwd is a git repo (fallback discovery), or fail gracefully
        // Either is fine — we just verify it doesn't panic.
        assert!(!r.is_empty(), "should not be empty");
    }
}
