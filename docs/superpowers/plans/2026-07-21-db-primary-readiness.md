# DB Primary Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Turso promotion safe by detecting mirror divergence, preserving failed writes durably, and gating DB-primary reads on a clean recovery audit.

**Architecture:** JSONL/TOML remains authoritative during this preparatory release. Each session gains a canonical, versioned snapshot manifest and a durable file outbox; Turso receives the same manifest and snapshot atomically. A read-only audit compares both stores by canonical fingerprints, and a readiness gate rejects DB-primary promotion until every mirror is current and no outbox remains.

**Tech Stack:** Rust, serde/serde_json, SHA-256 (`sha2`), Turso/libSQL local databases, Tauri commands, Vitest.

## Global Constraints

- Do not enable DB-primary reading until the readiness gate passes.
- Keep existing JSONL/TOML data readable without a migration flag.
- Audit commands must not create, mutate, or repair data.
- Failed database writes must be recoverable after process restart.
- Preserve existing user worktree changes and use `apply_patch` for edits.

---

## File Structure

- `crates/deepx-session/src/mirror.rs`: canonical snapshot, SHA-256 manifest, durable outbox representation.
- `crates/deepx-session/src/store/turso_backend.rs`: manifest schema and atomic snapshot write/read APIs.
- `crates/deepx-session/src/manager.rs`: write-through/outbox/reconciliation/readiness orchestration.
- `crates/deepx-session/Cargo.toml`: direct SHA-256 dependency.
- `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/config.rs`: audit, reconciliation, and readiness diagnostic commands.
- `crates/deepx-tauri/src-tauri/src/lib.rs`: command registration.

### Task 1: Versioned canonical session snapshots

**Files:**
- Create: `crates/deepx-session/src/mirror.rs`
- Modify: `crates/deepx-session/src/lib.rs`
- Modify: `crates/deepx-session/Cargo.toml`
- Test: `crates/deepx-session/src/mirror.rs`

**Interfaces:**
- Produces `MirrorSnapshot::new(meta: SessionMeta, messages: Vec<Message>) -> Self`.
- Produces `MirrorManifest { schema_version: u32, revision: u64, meta_sha256: String, messages_sha256: String }`.
- Produces `MirrorSnapshot::manifest(revision: u64) -> MirrorManifest`.

- [ ] **Step 1: Write failing canonicalization tests**

```rust
#[test]
fn equal_snapshots_have_equal_manifest_hashes() {
    let snapshot = MirrorSnapshot::new(SessionMeta::new("seed"), vec![Message::user("hi")]);
    assert_eq!(snapshot.manifest(7), snapshot.manifest(7));
}
```

- [ ] **Step 2: Run the focused test**

Run: `cargo test -p deepx-session mirror::tests::equal_snapshots_have_equal_manifest_hashes`
Expected: FAIL because `mirror` does not exist.

- [ ] **Step 3: Implement canonical JSON hashing**

```rust
pub const MIRROR_SCHEMA_VERSION: u32 = 1;
pub fn sha256_json<T: Serialize>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", sha2::Sha256::digest(bytes)))
}
```

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p deepx-session mirror::tests`
Expected: PASS.

### Task 2: Turso manifest and atomic snapshot persistence

**Files:**
- Modify: `crates/deepx-session/src/store/turso_backend.rs`
- Test: `crates/deepx-session/src/store/turso_backend.rs`

**Interfaces:**
- Consumes `MirrorSnapshot` and `MirrorManifest`.
- Produces `TursoBackend::replace_snapshot(&self, snapshot: &MirrorSnapshot, manifest: &MirrorManifest) -> Result<(), String>`.
- Produces `TursoBackend::load_manifest(&self, seed: &str) -> Result<Option<MirrorManifest>, String>`.

- [ ] **Step 1: Write a failing round-trip test**

```rust
assert_eq!(db.load_manifest("seed")?.unwrap(), snapshot.manifest(1));
```

- [ ] **Step 2: Run it**

Run: `cargo test -p deepx-session turso_backend::tests::snapshot_manifest_round_trip`
Expected: FAIL because the API/table is absent.

- [ ] **Step 3: Add a `session_mirrors` table and transaction**

```sql
CREATE TABLE IF NOT EXISTS session_mirrors (
  seed TEXT PRIMARY KEY, schema_version INTEGER NOT NULL, revision INTEGER NOT NULL,
  meta_sha256 TEXT NOT NULL, messages_sha256 TEXT NOT NULL
)
```

Use one transaction to replace message rows, upsert metadata, and upsert the manifest.

- [ ] **Step 4: Run backend tests**

Run: `cargo test -p deepx-session turso_backend::tests`
Expected: PASS.

### Task 3: Durable file outbox and reconciliation

**Files:**
- Modify: `crates/deepx-session/src/mirror.rs`
- Modify: `crates/deepx-session/src/manager.rs`
- Test: `crates/deepx-session/src/manager.rs`

**Interfaces:**
- Produces `SessionManager::reconcile_mirror(&self, seed: &str) -> Result<(), String>`.
- Produces `SessionManager::reconcile_all_mirrors(&self) -> Vec<(String, Result<(), String>)>`.
- A failed DB snapshot write persists `<session>/.mirror-outbox.json`; successful replay removes it.

- [ ] **Step 1: Write the failure/restart test**

```rust
manager.write_outbox_for_test(&snapshot, 1)?;
manager.reconcile_mirror("seed")?;
assert!(!manager.outbox_path("seed").exists());
assert!(manager.audit_mirror("seed").consistent);
```

- [ ] **Step 2: Run it**

Run: `cargo test -p deepx-session manager::tests::reconcile_replays_durable_outbox`
Expected: FAIL because reconciliation is absent.

- [ ] **Step 3: Implement atomic outbox writes and replay**

Write JSON to a sibling temporary file, rename it to `.mirror-outbox.json`, then attempt `replace_snapshot`. Remove the outbox only after Turso reports success. Reconciliation reads the outbox first; otherwise it builds a snapshot from JSONL/meta.

- [ ] **Step 4: Run manager tests**

Run: `cargo test -p deepx-session manager::tests`
Expected: PASS.

### Task 4: Upgrade audit and DB-primary readiness gate

**Files:**
- Modify: `crates/deepx-session/src/manager.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/config.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/lib.rs`
- Test: `crates/deepx-session/src/manager.rs`

**Interfaces:**
- Produces `DbPrimaryReadiness { ready: bool, sessions: Vec<MirrorAudit>, pending_outboxes: Vec<String>, reasons: Vec<String> }`.
- Produces `SessionManager::db_primary_readiness(&self) -> DbPrimaryReadiness`.
- Produces Tauri commands `cmd_reconcile_turso_mirrors` and `cmd_check_db_primary_readiness`.

- [ ] **Step 1: Write failing readiness tests**

```rust
let readiness = manager.db_primary_readiness();
assert!(!readiness.ready);
assert_eq!(readiness.pending_outboxes, vec!["seed"]);
```

- [ ] **Step 2: Run them**

Run: `cargo test -p deepx-session manager::tests::db_primary_readiness_rejects_pending_outbox`
Expected: FAIL because the readiness API is absent.

- [ ] **Step 3: Implement a non-mutating gate**

Reject when an audit has missing data, schema/hash/revision mismatch, errors, or a durable outbox. The command serializes structured diagnostics; it must never turn on `turso_enabled` or alter read precedence.

- [ ] **Step 4: Run session and Tauri checks**

Run: `cargo test -p deepx-session && cargo check -p deepx-tauri`
Expected: PASS.

### Task 5: Real engineering test matrix and release decision

**Files:**
- Modify: `migration-report.md`
- Test: `crates/deepx-session/src/manager.rs`

- [ ] **Step 1: Add explicit recovery scenarios**

Cover normal JSONL+DB parity, stale DB message, missing JSONL, missing DB, pending outbox, successful replay, and a fresh-manager restart.

- [ ] **Step 2: Execute the full matrix**

Run: `cargo test -p deepx-config -p deepx-session; cargo check -p deepx-tauri; pnpm test:run -- SettingsView.test.tsx`
Expected: all task-related tests pass; record unrelated baseline failures separately.

- [ ] **Step 3: Record the promotion decision**

Document exact command output, known baseline failures, and whether `cmd_check_db_primary_readiness` is green on real `.deepx` data. Leave DB-primary disabled if any session needs repair.

## Self-Review

- Spec coverage: Tasks 1-2 provide stable version/fingerprint state; Task 3 provides durable write compensation; Task 4 provides the promotion/audit gate; Task 5 executes the requested real recovery matrix.
- Placeholder scan: no TBD/TODO or unspecified test steps remain.
- Type consistency: `MirrorSnapshot` and `MirrorManifest` are produced in Task 1, consumed in Tasks 2-3, and surfaced through `MirrorAudit`/`DbPrimaryReadiness` in Task 4.
