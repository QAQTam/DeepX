# Permission Level Selector Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persisted permission-level selector shared by the composer and Settings.

**Architecture:** `App` owns one permission-level signal loaded from existing config. A small reusable selector emits changes through a dedicated validated Tauri command, and both UI locations consume the same signal.

**Tech Stack:** SolidJS, TypeScript, Tauri 2, Rust, Vitest

## Global Constraints

- Only levels 1 through 4 are accepted.
- Level 4 must have danger styling.
- Changing level persists immediately.
- Do not expose or rewrite stored secrets.

---

### Task 1: Permission selector and persistence

**Files:**
- Create: `crates/deepx-tauri/src/components/composer/PermissionLevelSelect.tsx`
- Test: `crates/deepx-tauri/src/components/composer/PermissionLevelSelect.test.tsx`
- Modify: `crates/deepx-tauri/src-tauri/src/agent_bridge/commands/config.rs`
- Modify: `crates/deepx-tauri/src-tauri/src/lib.rs`
- Modify: `crates/deepx-tauri/src/App.tsx`
- Modify: `crates/deepx-tauri/src/components/ChatView.tsx`
- Modify: `crates/deepx-tauri/src/components/composer/ComposerDock.tsx`
- Modify: `crates/deepx-tauri/src/components/SettingsView.tsx`
- Modify: composer/settings styles and i18n dictionaries

**Interfaces:**
- Produces: `PermissionLevelSelect(props: { level: number; onChange(level: number): void | Promise<void>; compact?: boolean })`
- Produces: `cmd_set_permission_level(level: u8) -> Result<(), String>`

- [x] **Step 1: Write a failing selector test**

Assert four labeled options, callback value `3`, and danger class at level `4`.

- [x] **Step 2: Verify RED**

Run `npm run test:run -- src/components/composer/PermissionLevelSelect.test.tsx`; expect failure because the component is absent.

- [x] **Step 3: Implement the minimal component and backend command**

Validate `level` with `(1..=4).contains(&level)`, persist `cfg.permission_level`, save, and broadcast `Ui2Agent::ReloadConfig`.

- [x] **Step 4: Wire the shared signal**

Load `permission_level` through `cmd_load_config`, persist changes with `cmd_set_permission_level`, and pass the same value/callback to Composer and Settings.

- [x] **Step 5: Verify GREEN and regression suite**

Run the targeted test, `npm run test:run`, `npm run build`, and a targeted Rust test/check.

- [x] **Step 6: Commit**

Commit only the permission selector implementation, tests, spec, and plan.
