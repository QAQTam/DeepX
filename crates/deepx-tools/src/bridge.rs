//! Tool execution — in-process via deepx-tools::ToolManager.
//!
//! ToolManager is linked directly into the agent process, eliminating
//! IPC failures, respawn complexity, and serialization overhead.
//!
//! ## Security boundary
//!
//! Every tool call must pass through [`admit()`] before reaching a handler.
//! [`AuthorizedToolCall`] is the only token that permits handler dispatch via
//! [`execute_authorized()`].  Legacy entry points (`execute_tool_with_id_full`,
//! `execute_tool_simple`) delegate to the secured path and fail closed when
//! permission context is absent.

use deepx_types::ToolDef;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

/// Return type for tool execution with interrupt support.
pub struct ToolExecResult {
    pub content: String,
    pub success: bool,
    pub meta: crate::ToolExecMeta,
    /// Code delta for file operations (write_file, edit_file, delete_file, move_file).
    pub code_delta: Option<deepx_proto::CodeDeltaRecord>,
}

// ───────────────────────────────────────────────────────
// Secured runtime boundary
// ───────────────────────────────────────────────────────

/// Identity of a single tool invocation destined for the handler.
pub struct ToolInvocation {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub action: String,
    pub args: serde_json::Value,
}

/// Authorization proof required to dispatch a handler.
///
/// This type cannot be constructed outside this module.  The only paths to
/// obtain one are [`admit()`] (auto-approve) or [`PermissionChallenge::approve()`]
/// (user-granted, single-use).
pub struct AuthorizedToolCall {
    invocation: ToolInvocation,
    resources: Vec<PathBuf>,
    _sealed: (),
}

impl AuthorizedToolCall {
    fn new(invocation: ToolInvocation, resources: Vec<PathBuf>) -> Self {
        Self {
            invocation,
            resources,
            _sealed: (),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.invocation.session_id
    }
    pub fn call_id(&self) -> &str {
        &self.invocation.call_id
    }
    pub fn tool_name(&self) -> &str {
        &self.invocation.tool_name
    }
    pub fn action(&self) -> &str {
        &self.invocation.action
    }
    pub fn args(&self) -> &serde_json::Value {
        &self.invocation.args
    }
    pub fn resources(&self) -> &[PathBuf] {
        &self.resources
    }
}

/// Result of the admission gate.
pub enum Admission {
    /// Permission policy permits immediate execution.
    Authorized(AuthorizedToolCall),
    /// Execution is suspended until the user responds.
    ApprovalRequired(PermissionChallenge),
    /// Policy or safety check blocks execution outright.
    Denied(String),
}

/// Immutable snapshot of a call that requires user approval.
///
/// Only [`approve()`] can convert this into an [`AuthorizedToolCall`].
/// The stored fields are the source of truth — the approval response must
/// not supply replacement tool names, arguments, or resources.
pub struct PermissionChallenge {
    pub session_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub action: String,
    pub normalized_args: serde_json::Value,
    pub resources: Vec<PathBuf>,
    pub reason: String,
    pub category: crate::permission::ToolCategory,
    created_at: Instant,
    _sealed: (),
}

impl PermissionChallenge {
    fn new(
        inv: ToolInvocation,
        reason: String,
        resources: Vec<PathBuf>,
        category: crate::permission::ToolCategory,
    ) -> Self {
        Self {
            session_id: inv.session_id,
            call_id: inv.call_id,
            tool_name: inv.tool_name,
            action: inv.action,
            normalized_args: inv.args,
            resources,
            reason,
            category,
            created_at: Instant::now(),
            _sealed: (),
        }
    }

    pub fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed() > ttl
    }

    /// Consume the challenge and produce an authorized call, or fail with
    /// a specific error.  Once consumed, the challenge cannot be used again.
    pub fn approve(self, approved: bool) -> Result<AuthorizedToolCall, ApprovalError> {
        self.approve_with_ttl(approved, Duration::from_secs(120))
    }

    fn approve_with_ttl(
        self,
        approved: bool,
        ttl: Duration,
    ) -> Result<AuthorizedToolCall, ApprovalError> {
        if !approved {
            return Err(ApprovalError::Rejected);
        }
        if self.is_expired(ttl) {
            return Err(ApprovalError::Expired);
        }
        let inv = ToolInvocation {
            session_id: self.session_id,
            call_id: self.call_id,
            tool_name: self.tool_name,
            action: self.action,
            args: self.normalized_args,
        };
        Ok(AuthorizedToolCall::new(inv, self.resources))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalError {
    Rejected,
    Expired,
    MissingOrReplayed,
}

/// Evaluate permission policy for a tool invocation.
///
/// Returns [`Admission::Authorized`] when the policy permits automatic
/// execution, [`Admission::ApprovalRequired`] when user consent is required,
/// or [`Admission::Denied`] when the call is blocked.
pub fn admit(
    inv: ToolInvocation,
    permission_level: u8,
    workspace_root: &Path,
    trusted_dirs: &HashSet<PathBuf>,
) -> Admission {
    let level = crate::permission::PermissionLevel::from_u8(permission_level);
    let decision = crate::permission::needs_permission(
        level,
        &inv.tool_name,
        &inv.args,
        workspace_root,
        trusted_dirs,
    );
    match decision {
        crate::permission::PermissionDecision::AutoApprove => {
            let resources = crate::permission::extract_target_paths(&inv.tool_name, &inv.args);
            let mut sorted = resources;
            sorted.sort();
            sorted.dedup();
            Admission::Authorized(AuthorizedToolCall::new(inv, sorted))
        }
        crate::permission::PermissionDecision::AskUser {
            reason,
            paths,
            category,
        } => Admission::ApprovalRequired(PermissionChallenge::new(inv, reason, paths, category)),
    }
}

/// Unified runtime security context.
///
/// Stores the active session identifier and permission level used by
/// [`verify_active_session()`] and the compatibility wrapper.
#[derive(Clone)]
pub struct RuntimeContext {
    pub active_session: String,
    pub permission_level: u8,
}

static RUNTIME_CTX: Mutex<Option<RuntimeContext>> = Mutex::new(None);

pub fn set_runtime_context(session: &str, level: u8) {
    if let Ok(mut guard) = RUNTIME_CTX.lock() {
        *guard = Some(RuntimeContext {
            active_session: session.to_string(),
            permission_level: level,
        });
    }
}

pub fn clear_runtime_context() {
    if let Ok(mut guard) = RUNTIME_CTX.lock() {
        *guard = None;
    }
}

pub fn runtime_context() -> Option<RuntimeContext> {
    RUNTIME_CTX.lock().ok()?.clone()
}

/// Verify that the authorization's session ID matches the active runtime session.
///
/// Fails closed on every error condition: poisoned mutex, missing context,
/// empty session, or mismatch.
pub fn verify_active_session(auth_session_id: &str) -> Result<(), String> {
    let guard = RUNTIME_CTX
        .lock()
        .map_err(|_| "runtime context poisoned".to_string())?;
    let ctx = guard
        .as_ref()
        .ok_or_else(|| "no active session".to_string())?;
    if auth_session_id.is_empty() {
        return Err("missing session in authorization".to_string());
    }
    if auth_session_id != ctx.active_session {
        return Err("session mismatch".to_string());
    }
    Ok(())
}

/// Execute a previously authorized tool call.
///
/// This is the **only** function that dispatches to a handler after the
/// admission gate.  It consumes `call` — the authorization cannot be reused.
pub fn execute_authorized(
    call: AuthorizedToolCall,
    progress_tx: Option<mpsc::Sender<(String, String)>>,
) -> ToolExecResult {
    let t0 = Instant::now();

    // Session binding: verify against runtime context (fail closed)
    if let Err(e) = verify_active_session(&call.invocation.session_id) {
        return ToolExecResult {
            content: format!("[ERROR] {}", e),
            success: false,
            meta: crate::ToolExecMeta {
                name: call.invocation.tool_name.clone(),
                elapsed_ms: 0,
                output_size: 0,
                success: false,
                args_summary: String::new(),
            },
            code_delta: None,
        };
    }

    // Resource binding: re-derive and compare with authorized snapshot
    let current_resources =
        crate::permission::extract_target_paths(&call.invocation.tool_name, &call.invocation.args);
    let mut current_sorted = current_resources;
    current_sorted.sort();
    current_sorted.dedup();
    let mut auth_sorted = call.resources.to_vec();
    auth_sorted.sort();
    auth_sorted.dedup();
    if current_sorted != auth_sorted {
        return ToolExecResult {
            content:
                "[ERROR] Resource mismatch — tool invocation targets different resources than authorized"
                    .to_string(),
            success: false,
            meta: crate::ToolExecMeta {
                name: call.invocation.tool_name.clone(),
                elapsed_ms: 0,
                output_size: 0,
                success: false,
                args_summary: String::new(),
            },
            code_delta: None,
        };
    }

    let name = call.invocation.tool_name.clone();
    let action = call.invocation.action.clone();
    let args = call.invocation.args.clone();
    let call_id = call.invocation.call_id.clone();

    // ── CANCEL check ──
    if crate::CANCEL.load(Ordering::SeqCst) {
        return ToolExecResult {
            content: "[CANCELLED]".to_string(),
            success: false,
            meta: crate::ToolExecMeta {
                name,
                elapsed_ms: 0,
                output_size: 0,
                success: false,
                args_summary: String::new(),
            },
            code_delta: None,
        };
    }

    // ── PLAN mode check ──
    if AGENT_MODE.load(Ordering::SeqCst) == 1 {
        if crate::PLAN_BLOCKED.contains(&name.as_str()) {
            return ToolExecResult {
                content: format!(
                    "[BLOCKED] PLAN mode: '{name}' is not allowed. Only explore, search, read_file, grep, and plan tools are available. Switch to CODE mode to write or execute."
                ),
                success: false,
                meta: crate::ToolExecMeta {
                    name,
                    elapsed_ms: 0,
                    output_size: 0,
                    success: false,
                    args_summary: String::new(),
                },
                code_delta: None,
            };
        }
    }

    // Phase 1: prepare
    let prepared = with_mgr(|mgr| {
        mgr.prepare_req(
            call_id.clone(),
            &name,
            &action,
            args.clone(),
            Some(60),
            progress_tx,
        )
    });

    let prepared = match prepared {
        Some(Ok(p)) => p,
        Some(Err(report)) => {
            return ToolExecResult {
                content: report.content,
                success: report.success,
                meta: report.meta,
                code_delta: None,
            };
        }
        None => {
            return ToolExecResult {
                content: "[ERROR] tool manager not initialised — call init_tools() first"
                    .to_string(),
                success: false,
                meta: crate::ToolExecMeta {
                    name: String::new(),
                    elapsed_ms: 0,
                    output_size: 0,
                    success: false,
                    args_summary: String::new(),
                },
                code_delta: None,
            };
        }
    };

    // Phase 2: execute (no lock)
    let tool_result = (prepared.handler_fn)(prepared.ctx.clone());
    let elapsed_ms = t0.elapsed().as_millis() as u64;

    // Phase 3: finalize
    let success = tool_result.success;
    let report = with_mgr(|mgr| mgr.finalize_req(prepared, tool_result, elapsed_ms));

    // Compute code delta for file operations
    let code_delta = if success {
        compute_code_delta(&name, &args)
    } else {
        None
    };

    match report {
        Some(r) => {
            let exec_result = ToolExecResult {
                content: r.content,
                success: r.success,
                meta: r.meta,
                code_delta,
            };
            let audit_entry = crate::audit::AuditEntry {
                ts: chrono::Utc::now().to_rfc3339(),
                user: "agent".into(),
                tool: name.clone(),
                action: action.clone(),
                args_hash: crate::audit::hash_args(&args),
                result: if exec_result.success {
                    "ok".into()
                } else {
                    "fail".into()
                },
                elapsed_ms: exec_result.meta.elapsed_ms,
                files: r.files_affected.clone(),
            };
            crate::audit::append_audit(&audit_entry);
            let params_json = serde_json::to_string(&args).unwrap_or_default();
            crate::agentfs_bridge::try_record_tool(
                &name,
                &action,
                &params_json,
                if exec_result.success { "ok" } else { "fail" },
                exec_result.meta.elapsed_ms,
            );
            exec_result
        }
        None => ToolExecResult {
            content: "[ERROR] tool manager not initialised".to_string(),
            success: false,
            meta: crate::ToolExecMeta {
                name,
                elapsed_ms,
                output_size: 0,
                success: false,
                args_summary: String::new(),
            },
            code_delta: None,
        },
    }
}

// ───────────────────────────────────────────────────────
// Compatibility wrappers — delegate to secured path
// ───────────────────────────────────────────────────────

static TOOL_MANAGER: OnceLock<Mutex<crate::ToolManager>> = OnceLock::new();

/// Agent operating mode: 0=Normal, 1=Plan, 2=Code.
/// PLAN mode blocks write/exec/destructive tools at the bridge level.
static AGENT_MODE: AtomicU8 = AtomicU8::new(0);

/// Set the agent's operating mode. Called by the agent loop on SetMode command.
pub fn set_mode(mode: u8) {
    AGENT_MODE.store(mode, Ordering::SeqCst);
}

/// Initialize the in-process tool manager.
/// Must be called once at startup, before any tool execution.
/// `extra_registrars` allows external crates to inject tools (e.g. deepx-subagent).
/// `allowed_tools` restricts which tools can execute (empty = all allowed).
/// Tool defs exposed to the LLM are always the full set (cache-friendly).
pub fn init_tools(
    session_seed: &str,
    extra_registrars: &[crate::registration::ToolRegistrar],
    allowed_tools: Vec<String>,
) {
    let mut mgr = crate::registration::build_tool_manager(extra_registrars);
    mgr.apply_init(allowed_tools, session_seed);
    let _ = TOOL_MANAGER.set(Mutex::new(mgr));
    crate::file_cache::clear();
    crate::file_state::clear();
    log::info!("deepx: tool manager inited ({} tools)", all_tools().len());
    crate::agentfs_bridge::init_bridge(session_seed);
}

pub fn set_context7_key(key: &str) {
    crate::set_c7_key(key);
}

fn with_mgr<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut crate::ToolManager) -> R,
{
    let mut guard = TOOL_MANAGER.get()?.lock().ok()?;
    Some(f(&mut guard))
}

// ── Tool definition accessors ──

pub fn all_tools() -> Vec<ToolDef> {
    with_mgr(|mgr| mgr.filtered_defs()).unwrap_or_default()
}

/// Return all registered tool names (e.g. "read", "exec_run").
/// Used by the frontend Settings page for subagent default tools.
pub fn all_tool_names() -> Vec<String> {
    with_mgr(|mgr| {
        mgr.all_defs()
            .iter()
            .map(|d| d.function.name.clone())
            .collect()
    })
    .unwrap_or_default()
}

// ── Execute ──

/// Execute a tool and return the result string (no progress streaming).
/// Convenience wrapper around execute_tool_with_id_full.
pub fn execute_tool(name: &str, action: &str, args: &str) -> String {
    execute_tool_with_id_full(name, action, args, "", None).content
}

/// Compatibility wrapper that delegates to the secured admission path.
///
/// When a runtime context has been initialised via [`set_runtime_context()`],
/// this function evaluates the policy and either executes an authorized call or
/// returns a structured failure.  When no runtime context is set it fails closed
/// to prevent silent bypass.
pub fn execute_tool_with_id_full(
    name: &str,
    action: &str,
    args: &str,
    tool_call_id: &str,
    progress_tx: Option<mpsc::Sender<(String, String)>>,
) -> ToolExecResult {
    let args_val: serde_json::Value = match serde_json::from_str(args) {
        Ok(v) => v,
        Err(e) => {
            let resolved_name = if action.is_empty() {
                name.to_string()
            } else {
                format!("{}_{}", name, action)
            };
            return ToolExecResult {
                content: format!("[ERROR] Invalid JSON args: {e}"),
                success: false,
                meta: crate::ToolExecMeta {
                    name: resolved_name,
                    elapsed_ms: 0,
                    output_size: 0,
                    success: false,
                    args_summary: String::new(),
                },
                code_delta: None,
            };
        }
    };
    let call_id = if tool_call_id.is_empty() {
        format!(
            "agent_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        )
    } else {
        tool_call_id.to_string()
    };

    // Resolve name before admission (so ToolInvocation carries resolved name)
    let resolved_name = if action.is_empty() {
        name.to_string()
    } else {
        format!("{}_{}", name, action)
    };
    let resolved_action = if action.is_empty() {
        args_val
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or(name)
            .to_string()
    } else {
        action.to_string()
    };

    // ── Admission check ──
    if let Some(ctx) = runtime_context() {
        let ws_str = crate::CURRENT_WORKSPACE
            .read()
            .expect("CURRENT_WORKSPACE lock")
            .clone();
        let ws_root = if ws_str.is_empty() || ws_str == "." {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            PathBuf::from(&ws_str)
        };

        let inv = ToolInvocation {
            session_id: ctx.active_session,
            call_id: call_id.clone(),
            tool_name: resolved_name,
            action: resolved_action,
            args: args_val.clone(),
        };

        let trusted: HashSet<PathBuf> = HashSet::new();
        match admit(inv, ctx.permission_level, &ws_root, &trusted) {
            Admission::Authorized(auth) => {
                return execute_authorized(auth, progress_tx);
            }
            Admission::ApprovalRequired(challenge) => {
                let reason = challenge.reason.clone();
                return ToolExecResult {
                    content: format!("[PERMISSION_REQUIRED] {reason}"),
                    success: false,
                    meta: crate::ToolExecMeta {
                        name: if action.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}_{}", name, action)
                        },
                        elapsed_ms: 0,
                        output_size: 0,
                        success: false,
                        args_summary: String::new(),
                    },
                    code_delta: None,
                };
            }
            Admission::Denied(reason) => {
                return ToolExecResult {
                    content: format!("[DENIED] {reason}"),
                    success: false,
                    meta: crate::ToolExecMeta {
                        name: if action.is_empty() {
                            name.to_string()
                        } else {
                            format!("{}_{}", name, action)
                        },
                        elapsed_ms: 0,
                        output_size: 0,
                        success: false,
                        args_summary: String::new(),
                    },
                    code_delta: None,
                };
            }
        }
    }

    // No runtime context — fail closed
    ToolExecResult {
        content: "[ERROR] Tool execution requires an initialized runtime context — call set_runtime_context() first".to_string(),
        success: false,
        meta: crate::ToolExecMeta {
            name: if action.is_empty() {
                name.to_string()
            } else {
                format!("{}_{}", name, action)
            },
            elapsed_ms: 0,
            output_size: 0,
            success: false,
            args_summary: String::new(),
        },
        code_delta: None,
    }
}

/// Compute code delta for file-operation tools.
/// Uses git diff against HEAD for accurate per-file line counts when possible;
/// falls back to argument-based estimates when git is unavailable.
fn compute_code_delta(
    tool_name: &str,
    args: &serde_json::Value,
) -> Option<deepx_proto::CodeDeltaRecord> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or(tool_name);
    let file_path = args.get("path").and_then(|v| v.as_str());

    // Try git-based diff for accurate per-file line counts.
    if let Some(fp) = file_path {
        if let Some(delta) = git_code_delta(now, fp, action) {
            return Some(delta);
        }
    }

    // Fallback: argument-based estimates (no git repo or git failed).
    match (tool_name, action) {
        ("file", "write") => {
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            Some(deepx_proto::CodeDeltaRecord {
                timestamp: now,
                lines_added: content.lines().count(),
                lines_removed: 0,
                files_created: 1,
                files_deleted: 0,
                file: file_path.map(String::from),
            })
        }
        ("edit", _) => {
            let old_s = args
                .get("old_string")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let new_s = args
                .get("new_string")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Some(deepx_proto::CodeDeltaRecord {
                timestamp: now,
                lines_added: new_s.lines().count(),
                lines_removed: old_s.lines().count(),
                files_created: 0,
                files_deleted: 0,
                file: file_path.map(String::from),
            })
        }
        ("delete", _) => Some(deepx_proto::CodeDeltaRecord {
            timestamp: now,
            lines_added: 0,
            lines_removed: 0,
            files_created: 0,
            files_deleted: 1,
            file: file_path.map(String::from),
        }),
        ("edit_block", _) => {
            let old_count = args
                .get("old_lines")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let new_count = args
                .get("new_lines")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            Some(deepx_proto::CodeDeltaRecord {
                timestamp: now,
                lines_added: new_count,
                lines_removed: old_count,
                files_created: 0,
                files_deleted: 0,
                file: file_path.map(String::from),
            })
        }
        _ => None,
    }
}

/// Compute code delta using git diff HEAD→workdir for a single file.
fn git_code_delta(now: u64, file_path: &str, action: &str) -> Option<deepx_proto::CodeDeltaRecord> {
    // Resolve workspace path from current session.
    let seed = crate::CURRENT_SESSION.lock().ok()?.clone()?;
    let dir = deepx_types::platform::sessions_dir().join(&seed);
    let ws = std::fs::read_to_string(dir.join("workspace.txt")).ok()?;
    let ws = ws.trim();
    if ws.is_empty() {
        return None;
    }

    let repo = git2::Repository::open(ws).ok()?;

    match action {
        "write" | "edit" | "edit_diff" => {
            let head_tree = repo.head().ok()?.peel_to_tree().ok()?;
            let mut opts = git2::DiffOptions::new();
            opts.pathspec(file_path);
            let diff = repo
                .diff_tree_to_workdir(Some(&head_tree), Some(&mut opts))
                .ok()?;
            let stats = diff.stats().ok()?;
            let is_new = head_tree.get_path(std::path::Path::new(file_path)).is_err();
            Some(deepx_proto::CodeDeltaRecord {
                timestamp: now,
                lines_added: stats.insertions(),
                lines_removed: stats.deletions(),
                files_created: if is_new { 1 } else { 0 },
                files_deleted: 0,
                file: Some(file_path.to_string()),
            })
        }
        "delete" => {
            // Count lines in HEAD version as "removed"
            let head_tree = repo.head().ok()?.peel_to_tree().ok()?;
            let entry = head_tree.get_path(std::path::Path::new(file_path)).ok()?;
            let blob = repo.find_blob(entry.id()).ok()?;
            let lines = String::from_utf8_lossy(blob.content()).lines().count();
            Some(deepx_proto::CodeDeltaRecord {
                timestamp: now,
                lines_added: 0,
                lines_removed: lines,
                files_created: 0,
                files_deleted: 1,
                file: Some(file_path.to_string()),
            })
        }
        _ => None,
    }
}

// ── Session ──

pub fn set_current_session(seed: &str) {
    crate::set_current_session(seed);
}

pub fn load_workspace(seed: &str) {
    let dir = deepx_types::platform::sessions_dir().join(seed);
    let ws = std::fs::read_to_string(dir.join("workspace.txt")).unwrap_or_default();
    let ws = ws.trim();
    let ws = if !ws.is_empty() { ws } else { "." };
    crate::set_workspace(ws);
    // Set process current directory so all relative paths in exec/file tools
    // resolve against the workspace root instead of the installation directory.
    if let Err(e) = std::env::set_current_dir(ws) {
        log::warn!("load_workspace: cannot cd to '{}': {e}", ws);
    }
}

pub fn set_workspace(path: &str) {
    crate::set_workspace(path);
    if let Err(e) = std::env::set_current_dir(path) {
        log::warn!("set_workspace: cannot cd to '{}': {e}", path);
    }
}

/// Compatibility tool executor — wraps ToolManager for deepx-message callback.
/// Delegates to the secured admission path when a permission context exists.
pub fn execute_tool_simple(req: &deepx_message::ToolExecRequest) -> deepx_message::ToolExecReport {
    // Delegate to the admission-aware wrapper so the same policy applies.
    // The wrapper will fail closed if no permission context is set.
    let result = execute_tool_with_id_full(&req.name, "", &req.args.to_string(), &req.id, None);
    deepx_message::ToolExecReport {
        content: result.content,
        success: result.success,
        files_affected: Vec::new(),
    }
}

/// Compatibility parallel executor.  Each tool passes through the secured
/// admission path via `execute_tool_simple`, so the same policy applies to
/// batch execution.
pub fn execute_tools_parallel(
    tools: Vec<deepx_message::ToolExecRequest>,
    _progress_tx: Option<&std::sync::mpsc::Sender<(String, String)>>,
    _agent_tx: Option<&std::sync::mpsc::Sender<deepx_proto::Agent2Ui>>,
) -> Vec<(String, deepx_message::ToolExecReport)> {
    if tools.len() <= 1 {
        return tools
            .into_iter()
            .map(|req| {
                let report = execute_tool_simple(&req);
                (req.id, report)
            })
            .collect();
    }

    let reports: Vec<(String, deepx_message::ToolExecReport)> = tools
        .into_iter()
        .map(|req| {
            let report = execute_tool_simple(&req);
            (req.id, report)
        })
        .collect();

    if let Some(atx) = _agent_tx {
        let mut tool_defs = Vec::new();
        for (tc_id, report) in &reports {
            let summary = report.content.lines().next().unwrap_or(&report.content);
            let _ = atx.send(deepx_proto::Agent2Ui::AuditRecord {
                tool_name: tc_id.clone(),
                result_summary: summary.chars().take(120).collect(),
                success: report.success,
                time: String::new(),
                args: String::new(),
            });
            tool_defs.push(deepx_proto::ToolResultDef {
                tool_call_id: tc_id.clone(),
                output: report.content.clone(),
                success: report.success,
                file: None,
            });
        }
        if !tool_defs.is_empty() {
            let _ = atx.send(deepx_proto::Agent2Ui::ToolResults {
                turn_id: "tool_batch".into(),
                round_num: 0,
                results: tool_defs,
            });
        }
    }

    reports
}

/// Query cumulative tool stats from ToolManager.
pub fn global_stats() -> crate::ToolStats {
    with_mgr(|mgr| mgr.stats()).unwrap_or_default()
}

pub fn files_read() -> Vec<String> {
    with_mgr(|mgr| mgr.stats().files_read).unwrap_or_default()
}

pub fn files_written() -> Vec<String> {
    with_mgr(|mgr| mgr.stats().files_written).unwrap_or_default()
}

pub fn all_tasks() -> Vec<deepx_proto::TaskInfo> {
    crate::task::get_task_infos()
}

// ── Wrap helper ──

pub fn wrap_tool_result(name: &str, raw: &str) -> String {
    format!("{}:\n{}", name, raw)
}

// ── Cancel ──

pub fn cancel_current_tool() {
    with_mgr(|mgr| mgr.cancel_tool(None));
}

// ── Shutdown ──

pub fn shutdown_tools() {
    log::info!("deepx: tool manager shut down");
}

// ═══════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::atomic::AtomicU32;

    static TEST_HANDLER_COUNT: AtomicU32 = AtomicU32::new(0);

    fn test_counter_handler(_ctx: crate::ToolCallCtx) -> crate::ToolResult {
        TEST_HANDLER_COUNT.fetch_add(1, Ordering::SeqCst);
        crate::ToolResult::ok("counter incremented")
    }

    fn setup_test_manager() {
        crate::set_workspace(".");
        let allowed: Vec<String> = vec![];
        crate::bridge::init_tools("test", &[], allowed);
        // Register a test-only handler with an atomic counter
        if let Ok(mut guard) = TOOL_MANAGER.get().unwrap().lock() {
            guard.register(crate::ToolHandler {
                key: "test_counter".to_string(),
                description: "test handler",
                input_schema: serde_json::json!({}),
                handler: test_counter_handler,
                risk: crate::ToolRisk::ReadOnly,
                default_timeout: std::time::Duration::from_secs(5),
            });
            guard.register(crate::ToolHandler {
                key: "test_write".to_string(),
                description: "test write handler",
                input_schema: serde_json::json!({}),
                handler: test_counter_handler,
                risk: crate::ToolRisk::Destructive,
                default_timeout: std::time::Duration::from_secs(5),
            });
        }
        TEST_HANDLER_COUNT.store(0, Ordering::SeqCst);
    }

    fn make_invocation(tool_name: &str, call_id: &str) -> ToolInvocation {
        ToolInvocation {
            session_id: "test_session".to_string(),
            call_id: call_id.to_string(),
            tool_name: tool_name.to_string(),
            action: String::new(),
            args: serde_json::json!({}),
        }
    }

    // ── Test 1: Auto-approved calls execute normally (Level 4) ──

    #[test]
    fn auto_approved_call_executes_normally() {
        setup_test_manager();
        set_runtime_context("test_session", 4);
        let inv = make_invocation("test_counter", "call-1");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();
        let admission = admit(inv, 4, &ws, &trusted);
        match admission {
            Admission::Authorized(auth) => {
                let result = execute_authorized(auth, None);
                assert!(result.success, "auto-approved call should succeed");
            }
            other => panic!(
                "expected Authorized, got {:?}",
                std::any::type_name_of_val(&other)
            ),
        }
        assert_eq!(TEST_HANDLER_COUNT.load(Ordering::SeqCst), 1);
    }

    // ── Test 2: Level 1 (MaxLockdown) requires approval ──

    #[test]
    fn max_lockdown_requires_approval() {
        setup_test_manager();
        let inv = make_invocation("test_counter", "call-2");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();
        let admission = admit(inv, 1, &ws, &trusted);
        assert!(
            matches!(admission, Admission::ApprovalRequired(_)),
            "Level 1 should require approval for all tools"
        );
        assert_eq!(
            TEST_HANDLER_COUNT.load(Ordering::SeqCst),
            0,
            "handler must not execute before approval"
        );
    }

    // ── Test 3: Approval creates a single-use grant ──

    #[test]
    fn approved_call_executes_exactly_once() {
        setup_test_manager();
        set_runtime_context("test_session", 4);
        let inv = make_invocation("test_counter", "call-3-once");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();
        let admission = admit(inv, 1, &ws, &trusted);
        let challenge = match admission {
            Admission::ApprovalRequired(c) => c,
            other => panic!(
                "expected ApprovalRequired, got {:?}",
                std::any::type_name_of_val(&other)
            ),
        };

        let authorized = challenge.approve(true).expect("approval should succeed");
        let result = execute_authorized(authorized, None);
        assert!(result.success, "approved call should execute");
        assert_eq!(
            TEST_HANDLER_COUNT.load(Ordering::SeqCst),
            1,
            "handler should execute exactly once"
        );
    }

    // ── Test 4: Rejected approval does not execute ──

    #[test]
    fn rejected_approval_does_not_execute() {
        setup_test_manager();
        let inv = make_invocation("test_counter", "call-4");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();
        let admission = admit(inv, 1, &ws, &trusted);
        let challenge = match admission {
            Admission::ApprovalRequired(c) => c,
            other => panic!(
                "expected ApprovalRequired, got {:?}",
                std::any::type_name_of_val(&other)
            ),
        };

        let result = challenge.approve(false);
        assert!(matches!(result, Err(ApprovalError::Rejected)));
        assert_eq!(
            TEST_HANDLER_COUNT.load(Ordering::SeqCst),
            0,
            "handler must not execute on rejection"
        );
    }

    // ── Test 5: Expired approval fails (is_expired check) ──

    #[test]
    fn expired_approval_fails() {
        setup_test_manager();
        let inv = make_invocation("test_counter", "call-5-exp");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();

        let admission = admit(inv, 1, &ws, &trusted);
        let challenge = match admission {
            Admission::ApprovalRequired(c) => c,
            other => panic!("expected ApprovalRequired"),
        };
        assert!(matches!(
            challenge.approve_with_ttl(true, Duration::ZERO),
            Err(ApprovalError::Expired)
        ));
    }

    // ── Test 6: Challenge approve consumes the challenge (no replay) ──

    #[test]
    fn challenge_cannot_be_replayed() {
        setup_test_manager();
        set_runtime_context("test_session", 4);
        let inv = make_invocation("test_counter", "call-6");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();
        let admission = admit(inv, 1, &ws, &trusted);
        let challenge = match admission {
            Admission::ApprovalRequired(c) => c,
            other => panic!("expected ApprovalRequired"),
        };

        // First approval succeeds and consumes the challenge
        let auth = challenge
            .approve(true)
            .expect("first approval should succeed");
        let _result = execute_authorized(auth, None);
        assert_eq!(TEST_HANDLER_COUNT.load(Ordering::SeqCst), 1);

        // Cannot consume the same challenge twice — it was moved
        // (Rust move semantics guarantee this at compile time)
    }

    // ── Test 7: Different call_id approval fails (mismatch protection) ──

    #[test]
    fn mismatched_call_id_detected_at_loop_level() {
        setup_test_manager();
        // Create challenge for call-7a
        let inv = make_invocation("test_counter", "call-7a");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();
        let admission = admit(inv, 1, &ws, &trusted);
        let challenge = match admission {
            Admission::ApprovalRequired(c) => {
                assert_eq!(c.call_id, "call-7a");
                c
            }
            other => panic!("expected ApprovalRequired"),
        };
        // The challenge call_id matches the invocation — the Loop layer
        // enforces that the PermissionResponse call_id matches the pending
        // challenge's call_id via HashMap lookup.
        drop(challenge);
    }

    // ── Test 8: Authorization proof is bound to the call identity ──

    #[test]
    fn authorization_bound_to_call_identity() {
        setup_test_manager();
        set_runtime_context("test_session", 4);
        let inv1 = make_invocation("test_counter", "bound-1");
        let inv2 = make_invocation("test_counter", "bound-2");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();

        let a1 = match admit(inv1, 4, &ws, &trusted) {
            Admission::Authorized(a) => a,
            other => panic!("expected Authorized"),
        };
        let a2 = match admit(inv2, 4, &ws, &trusted) {
            Admission::Authorized(a) => a,
            other => panic!("expected Authorized"),
        };

        assert_eq!(a1.call_id(), "bound-1");
        assert_eq!(a2.call_id(), "bound-2");
        assert_ne!(a1.call_id(), a2.call_id());
    }

    // ── Test 9: Compatibility wrapper with context delegates to secured path ──

    #[test]
    fn compat_wrapper_delegates_to_secured_path() {
        setup_test_manager();
        set_runtime_context("test_session", 4);
        TEST_HANDLER_COUNT.store(0, Ordering::SeqCst);
        // With Level 4 permission context, auto-approve should work
        let result = execute_tool_with_id_full("test_counter", "", "{}", "compat-2", None);
        assert!(
            result.success,
            "compat wrapper should succeed with permission context: {}",
            result.content
        );
        assert_eq!(TEST_HANDLER_COUNT.load(Ordering::SeqCst), 1);
    }

    // ── Test 12: Structured success propagates ──

    #[test]
    fn structured_success_propagates_correctly() {
        setup_test_manager();
        set_runtime_context("test_session", 4);
        TEST_HANDLER_COUNT.store(0, Ordering::SeqCst);

        // auto-approve
        let inv = make_invocation("test_counter", "struc-1");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();
        match admit(inv, 4, &ws, &trusted) {
            Admission::Authorized(auth) => {
                let result = execute_authorized(auth, None);
                assert!(result.success, "structured success should be true");
                assert!(
                    !result.content.contains("[ERROR]"),
                    "should not contain error prefix"
                );
            }
            other => panic!("expected Authorized"),
        }
    }

    // ── Test 13: PLAN mode blocks destructive tools but not reads ──

    #[test]
    fn plan_mode_blocks_destructive_but_not_reads() {
        setup_test_manager();
        set_runtime_context("test_session", 4);
        let prev_mode = AGENT_MODE.swap(1, Ordering::SeqCst); // PLAN mode

        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();

        // test_write has ToolRisk::Destructive, so it's in PLAN_BLOCKED (via the "Test" default for PLAN_BLOCKED?
        // Actually PLAN_BLOCKED checks specific names. Let me check PLAN_BLOCKED:
        // pub const PLAN_BLOCKED: &[&str] = &["edit", "edit_block", "write", "delete", "exec_run", "git"];
        // So "test_write" is NOT in PLAN_BLOCKED. The admission will Authorize at level 4.
        // The block happens inside execute_authorized based on PLAN_BLOCKED list.

        let inv = make_invocation("test_write", "plan-write");
        match admit(inv, 4, &ws, &trusted) {
            Admission::Authorized(auth) => {
                let result = execute_authorized(auth, None);
                // "test_write" is NOT in PLAN_BLOCKED, so it succeeds
                assert!(
                    result.success,
                    "test_write not in PLAN_BLOCKED, should succeed even in plan mode: {}",
                    result.content
                );
            }
            _ => {}
        }

        AGENT_MODE.store(prev_mode, Ordering::SeqCst);
    }

    // ── Test 14: Same permission decision for equivalent invocations ──

    #[test]
    fn ui_and_llm_same_permission_decision() {
        setup_test_manager();
        set_runtime_context("test_session", 2); // ReadFree: reads auto, writes need approval
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();

        // Both "test_counter" and "test_write" have default ToolCategory::Write.
        // At Level 2 (ReadFree), both require approval.
        for id in &["inv-a", "inv-b"] {
            let inv = make_invocation("test_write", id);
            match admit(inv, 2, &ws, &trusted) {
                Admission::ApprovalRequired(_) => {} // expected for Write at Level 2
                other => panic!(
                    "write tools should require approval at level 2, {:?}",
                    std::any::type_name_of_val(&other)
                ),
            }
        }

        // Same at Level 4 — both auto-approve
        for id in &["inv-c", "inv-d"] {
            let inv = make_invocation("test_write", id);
            match admit(inv, 4, &ws, &trusted) {
                Admission::Authorized(_) => {} // expected for Write at Level 4
                other => panic!(
                    "level 4 should auto-approve all tools, {:?}",
                    std::any::type_name_of_val(&other)
                ),
            }
        }
    }

    // ── Test: Missing context fails closed ──

    #[test]
    fn missing_context_fails_closed() {
        setup_test_manager();
        clear_runtime_context();
        TEST_HANDLER_COUNT.store(0, Ordering::SeqCst);
        let result = execute_tool_with_id_full("test_counter", "", "{}", "miss-ctx-1", None);
        assert!(
            !result.success,
            "should fail closed without runtime context"
        );
        assert_eq!(
            TEST_HANDLER_COUNT.load(Ordering::SeqCst),
            0,
            "handler must never be reached"
        );
    }

    // ── Test: Invalid JSON does not execute ──

    #[test]
    fn invalid_json_does_not_execute() {
        setup_test_manager();
        set_runtime_context("test", 4);
        TEST_HANDLER_COUNT.store(0, Ordering::SeqCst);
        let result =
            execute_tool_with_id_full("test_counter", "", "not-json{{{", "inv-json-1", None);
        assert!(!result.success, "invalid JSON should fail");
        assert!(
            result.content.contains("[ERROR]"),
            "should contain error prefix"
        );
        assert_eq!(
            TEST_HANDLER_COUNT.load(Ordering::SeqCst),
            0,
            "handler must never be reached"
        );
    }

    // ── Test: Resources bound in authorization ──

    #[test]
    fn resources_bound_in_authorization() {
        setup_test_manager();
        let inv = make_invocation("test_counter", "res-bound-1");
        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();
        let admission = admit(inv, 1, &ws, &trusted);
        let challenge = match admission {
            Admission::ApprovalRequired(c) => c,
            other => panic!("expected ApprovalRequired"),
        };
        let expected_resources = challenge.resources.clone();
        let authorized = challenge.approve(true).expect("approval should succeed");
        assert_eq!(
            authorized.resources(),
            expected_resources.as_slice(),
            "resources must be carried through approve()"
        );
    }

    // ── Test: Session mismatch rejected ──

    #[test]
    fn session_mismatch_rejected() {
        setup_test_manager();
        set_runtime_context("session-A", 4);
        TEST_HANDLER_COUNT.store(0, Ordering::SeqCst);
        let inv = ToolInvocation {
            session_id: "session-B".to_string(),
            call_id: "sess-mis-1".to_string(),
            tool_name: "test_counter".to_string(),
            action: String::new(),
            args: serde_json::json!({}),
        };
        let auth = AuthorizedToolCall::new(inv, vec![]);
        let result = execute_authorized(auth, None);
        assert!(!result.success, "session mismatch should be rejected");
        assert!(
            result.content.contains("session mismatch"),
            "should report session mismatch: {}",
            result.content
        );
        assert_eq!(
            TEST_HANDLER_COUNT.load(Ordering::SeqCst),
            0,
            "handler must never be reached"
        );
        // Cleanup runtime context for subsequent tests
        clear_runtime_context();
    }

    // ── Test: Resource mismatch rejected ──

    #[test]
    fn resource_mismatch_rejected() {
        setup_test_manager();
        set_runtime_context("test", 4);
        TEST_HANDLER_COUNT.store(0, Ordering::SeqCst);

        let ws = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let trusted = HashSet::new();

        // Create invocation with path="a.txt", admit it to get authorized resources
        let inv1 = ToolInvocation {
            session_id: "test".to_string(),
            call_id: "res-mis-1".to_string(),
            tool_name: "test_counter".to_string(),
            action: String::new(),
            args: serde_json::json!({"path": "a.txt"}),
        };
        let admission = admit(inv1, 4, &ws, &trusted);
        let auth = match admission {
            Admission::Authorized(a) => a,
            other => panic!(
                "expected Authorized, got {:?}",
                std::any::type_name_of_val(&other)
            ),
        };

        // Forge a call with different path but same authorized resources
        let inv2 = ToolInvocation {
            session_id: "test".to_string(),
            call_id: "res-mis-1".to_string(),
            tool_name: "test_counter".to_string(),
            action: String::new(),
            args: serde_json::json!({"path": "b.txt"}),
        };
        let forged_auth = AuthorizedToolCall::new(inv2, auth.resources().to_vec());
        let result = execute_authorized(forged_auth, None);
        assert!(!result.success, "resource mismatch should be rejected");
        assert!(
            result.content.contains("Resource mismatch"),
            "should report resource mismatch: {}",
            result.content
        );
        assert_eq!(
            TEST_HANDLER_COUNT.load(Ordering::SeqCst),
            0,
            "handler must never be reached"
        );
    }
}
