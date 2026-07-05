# PLAN: Introduce `ts-rs` for TypeScript type generation

## Motivation

The Tauri frontend at `crates/deepx-tauri/src/` manually maintains ~12 TypeScript interfaces
mirroring Rust structs from `deepx-proto` and `deepx-types`. The Tauri event bridge (`Agent2Ui`)
dispatches 25+ event variants via `Record<string, unknown>` + unchecked `switch`, and all field
access uses `as` casts. This causes:

- **Type drift**: `ToolResultDef` in TS is missing `file?: FileSnapshotInfo` present in Rust
- **Duplicate definitions**: `CodeDelta` is defined twice identically in `store/chat.ts`
- **Naming mismatch**: TS types `Round`/`Turn`/`SessionInfo` use camelCase but the wire format is snake_case; `loadTurnsFromRestore()`/~30 lines of manual runtime remapping paper over the gap
- **`invoke<string>` + `JSON.parse` as `any`**: 8+ Tauri commands return untyped JSON strings

`ts-rs` replaces manual duplication with `#[derive(TS)]` → auto-generated `.ts` files,
making the Rust struct the single source of truth.

---

## Phase 1: Infrastructure (2 files, ~15 LOC)

### 1.1 Add `ts-rs` to `deepx-proto/Cargo.toml`

```toml
[dependencies]
ts-rs = { version = "10", features = ["serde-compat"] }
```

`serde-compat` enables `#[serde(tag = "type")]` → TS discriminated unions, `#[serde(rename)]`
→ TS property renaming.

### 1.2 Add `ts-rs` to `deepx-types/Cargo.toml`

Same dependency — needed for `UsageInfo`, `SessionMeta` (persisted fields only), `ToolDef`.

### 1.3 Add `ts-rs` to `src-tauri/Cargo.toml` (build-dependencies only)

```toml
[build-dependencies]
ts-rs = { version = "10", features = ["serde-compat"] }
```

The build.rs needs `ts-rs` at build time to orchestrate the export.

### 1.4 Update `src-tauri/build.rs`

```rust
fn main() {
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build();

    // Invoke ts-rs export. deepx-proto's lib.rs will re-export `TS`
    // and the build script triggers the export pass.
    // This is a no-op on first build; the actual export is driven by
    // `#[derive(TS)]` + `#[ts(export)]` annotations in the source crates.
}
```

> **Note**: The exact mechanism depends on the `ts-rs` version. In v10, `#[ts(export)]`
> on each type triggers file generation at compile time (via proc-macro). If using
> `ts-rs` v8, we'd use a `TS::export_all()` call in the crate's own lib.rs. We'll
> adopt whatever the latest stable API provides.

---

## Phase 2: Annotate Rust types (3 files, ~35 `#[derive(TS)]`)

### 2.1 `deepx-proto/src/agent_protocol.rs` — primary target

All public types crossing the UI↔Agent boundary:

| Type | Notes |
|------|-------|
| `Agent2Ui` | Tagged union, 25 variants — **highest value** |
| `Ui2Agent` | Tagged union, 10 variants — frontend sends these too |
| `RoundBlock` | Tagged union (reasoning/text/tool) |
| `RoundDeltaKind` | Simple enum |
| `ToolCallDef` | Struct — replaces manual TS `ToolCallDef` |
| `ToolResultDef` | Struct — replaces manual TS `ToolResultDef` |
| `FileSnapshotInfo` | Struct — currently missing from TS |
| `DocInfo` | Struct — used in Dashboard |
| `TaskInfo` | Struct — replaces manual TS `TaskInfo` |
| `RoundData` | Struct — replaces manual `Round` |
| `TurnData` | Struct — replaces manual `Turn` |
| `CodeDeltaRecord` | Struct — used for persistence |
| `CodeDaily` | Struct — used in StockChart |
| `FrontendToDaemon` | Internal — **skip**, daemon-only |
| `DaemonToFrontend` | Internal — **skip**, daemon-only |

### 2.2 `deepx-types/src/api_types.rs`

| Type | Notes |
|------|-------|
| `UsageInfo` | Used in `TurnEnd`, `Dashboard` — needed by frontend |

### 2.3 `deepx-types/src/session.rs`

| Type | Notes |
|------|-------|
| `SessionMeta` | **Partial export only**: persist fields (seed, model, created_at, updated_at, message_count, turn_count, last_summary, compact_skip). Skip runtime fields: `resume_seed`, `tokens`, `title`, `from_resume`. Use `#[ts(skip)]`. |

> **Decision: Skip `ToolDef`/`ToolFunction`** — these are backend-internal (LLM tool definitions,
> not surfaced to frontend). Frontend doesn't consume tool definitions directly.

### 2.4 Naming strategy

**Finding: the wire format is uniformly snake_case.** Rust struct fields use default serde
behavior (no `rename_all` on any protocol type), so JSON keys are snake_case: `tool_call_id`,
`round_num`, `user_text`, etc. Variant tags via `#[serde(rename = "...")]` are also snake_case:
`"turn_start"`, `"round_complete"`.

**Strategy: `ts-rs` generates snake_case TS by default.** No `rename_all` attribute needed.
The generated TS types will match the wire format exactly:

```typescript
// Generated from Agent2Ui (snake_case, matches wire)
export type Agent2Ui =
  | { type: "turn_start"; turn_id: string; user_text: string }
  | { type: "round_delta"; turn_id: string; round_num: number; kind: RoundDeltaKind; delta: string }
  | { type: "round_complete"; turn_id: string; round_num: number; thinking?: string; answer?: string; tool_calls: ToolCallDef[]; blocks: RoundBlock[]; is_final: boolean }
  // ... 22 more variants
```

**What gets fixed on the TS side:**
- `Round` (`roundNum`, `toolCalls`, `toolResults`) → replace with generated `RoundData` (`round_num`, `tool_calls`, `tool_results`)
- `Turn` (`turnId`, `userText`, `stopReason`) → replace with generated `TurnData` (`turn_id`, `user_text`, `stop_reason`)
- `SessionInfo` → keep as UI-only composite (no Rust struct), but rename fields to snake_case for consistency
- `loadTurnsFromRestore()` and `prependTurns()` become pass-through — **~30 lines deleted**

**Zero wire-format risk.** The JSON on the wire does not change. Old session files remain compatible.
The change is purely on the TS side: fix the types to match the wire, then delete the runtime conversion.

---

## Phase 3: Frontend integration (1 file new, ~5 files delta)

### 3.1 Generated output directory

```
crates/deepx-tauri/src/lib/types/
├── Agent2Ui.ts
├── Ui2Agent.ts
├── RoundBlock.ts
├── ToolCallDef.ts
├── ToolResultDef.ts
├── FileSnapshotInfo.ts
├── DocInfo.ts
├── TaskInfo.ts
├── RoundData.ts
├── TurnData.ts
├── CodeDeltaRecord.ts
├── CodeDaily.ts
├── UsageInfo.ts
├── SessionMeta.ts
└── index.ts            # barrel re-export
```

The barrel file (`index.ts`) re-exports all types so consumers do `import { Agent2Ui, ... } from "@/lib/types"`.

### 3.2 Replace manual types in `store/chat.ts`

**Remove** L5-29 (all manual `export interface` definitions) and replace with:

```typescript
import type { ToolCallDef, ToolResultDef, RoundBlock, TaskInfo, CodeDeltaRecord, TurnData, RoundData } from "@/lib/types";
```

Keep `SessionInfo`, `ActivityEntry`, `AskState` — these are UI-only or composite types with no Rust counterpart.

### 3.3 Update `App.tsx` event dispatch

Replace `Record<string, unknown>` with the generated `Agent2Ui` union:

```typescript
import type { Agent2Ui } from "@/lib/types";

function handleAgentEvent(chat: ChatStore, p: Agent2Ui, listenerSeed: string) {
  switch (p.type) {
    case "turn_start":  chat.handleTurnStart(p.turn_id, p.user_text); break;
    case "round_delta": chat.handleRoundDelta(p.turn_id, p.round_num, p.kind, p.delta); break;
    // ... all branches now type-safe, no `as` casts needed
  }
}
```

The `listen` call changes from `listen<Record<string, unknown>>` to `listen<Agent2Ui>`.

### 3.4 Update component imports

- `StockChart.tsx` — `import type { CodeDaily } from "@/lib/types"` replaces manual interface
- `GitDiffPanel.tsx` — `import type { CodeDaily } from "@/lib/types"`
- `ContextPanel.tsx` — keep its `ContextStats` interface (computed on Rust side, no struct)

### 3.5 Remove manual snake→camelCase conversion

`loadTurnsFromRestore()` (chat.ts:397-414) and `prependTurns()` (chat.ts:416-435)
manually convert snake_case JSON to camelCase TS objects because `Round` and `Turn`
use the wrong naming convention. After replacing these with the generated snake_case
types (`RoundData`, `TurnData`), both functions simplify to a direct assignment:

```typescript
// Before (30 lines of manual remapping)
function loadTurnsFromRestore(turnsData: Array<{...}>) {
  const loaded: Turn[] = turnsData.map((td) => {
    const rounds: Round[] = td.rounds.map((rd) => ({
      roundNum: rd.round_num,   // ← manual snake→camelCase
      toolCalls: rd.tool_calls,  // ← manual rename
      // ...
    }));
    return { turnId: td.turn_id, userText: td.user_text, ... };  // ← manual rename
  });
}

// After (3 lines, types match wire exactly)
function loadTurnsFromRestore(turnsData: TurnData[]) {
  setTurns(turnsData.map((td) => ({ ...td, status: "complete" as const })));
}
```

> **Note**: The `status` field is UI-only (not on the wire). Added here; `TurnData` from
> `ts-rs` won't include it. Alternative: use a separate `Turn` type that extends `TurnData`
> with `status`, keeping the UI concern separate.

---

## Phase 4: Validation & cleanup

### 4.1 Verify build

```bash
cd crates/deepx-tauri/src-tauri && cargo build  # ensures Rust compiles with TS derives
cd crates/deepx-tauri && pnpm tsc --noEmit       # ensures TS types are consistent
```

### 4.2 Delete duplicate `CodeDelta` in `store/chat.ts`

The second copy at L23-29 is removed by the import replacement.

### 4.3 Add `@/lib/types/` to `.gitignore` (?)

**Decision: Commit generated files.** Generated types are small (~2KB), checked-in types
let contributors browse the protocol without building Rust. CI can verify they're up-to-date
via `git diff --exit-code` after `cargo build`.

### 4.4 Documentation

Add a comment block at the top of `agent_protocol.rs`:
```rust
//! TypeScript types are auto-generated via ts-rs.
//! After modifying any struct/enum in this file, run `cargo build` in
//! `crates/deepx-tauri/src-tauri` to regenerate `../src/lib/types/`.
```

---

## Phase 5: Future improvements (out of scope for v0.6.1-alpha)

1. **Typed Tauri commands**: Replace `Result<String, String>` return pattern with
   `Result<SomeStruct, String>` and add `#[derive(TS)]` to the return structs.
   This eliminates `JSON.parse(as any)` at 8+ call sites.

2. **`specta` evaluation**: If all Tauri commands need full type safety (including
   parameter types), `tauri-specta` would generate typed `invoke()` bindings.
   This is a larger refactor but builds on the `ts-rs` foundation.

3. **CI check**: Add `cargo build && git diff --exit-code` to CI to ensure
   generated types stay in sync.

---

## Risk assessment

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `serde_json::Value` fields map to `any` | Certain | `Ui2Agent::ToolCall.args` → `any`. Acceptable — these are user-typed arguments, inherently dynamic |
| TS field rename (camelCase → snake_case) in `Round`/`Turn` | Medium | ~6 TS files reference `roundNum`/`toolCalls` etc. TypeScript compiler catches all mismatches at build time (`tsc --noEmit`) |
| `ts-rs` version churn | Low | Pin to `"10"` in Cargo.toml. Upgrade later when stable |
| Generated types go out of sync | Medium | Commit generated files to git; CI check enforces consistency |

---

## Estimated effort

| Phase | Files changed | Estimated LOC |
|-------|--------------|---------------|
| Phase 1: Infrastructure | 3 | +15 |
| Phase 2: Annotate types | 2 | +45 |
| Phase 3: Frontend integration | 5 | -80 / +25 |
| Phase 4: Validation | 3 | +10 |
| **Total** | **13** | **~+95 / -80** |
