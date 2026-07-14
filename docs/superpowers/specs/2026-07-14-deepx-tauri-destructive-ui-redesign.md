# DeepX Tauri Destructive UI Redesign

Date: 2026-07-14

Status: Approved design

Scope owner: `deepx-tauri`, with explicit protocol prerequisites in `deepx-proto` and the Rust permission producer.

## 1. Objective

Rebuild the DeepX desktop UI around a focused task transcript inspired by the information hierarchy of Codex, without copying Codex branding or proprietary assets.

The redesign is intentionally destructive at the visual layer. The current App shell, message hierarchy, tool cards, permanent status panel, top telemetry bar, and composer are replacement targets. Existing protocol facts, Tauri commands, session behavior, permission lifecycle, ask-user lifecycle, Git capabilities, Skills, Tasks, Plan Review, and streaming data remain functional.

The defining outcome is:

- While a turn is running, reasoning and tool activity appear inside one compact, expandable process trace.
- When a successful turn ends, the entire process trace collapses automatically to one `Processed in ...` disclosure.
- The default completed transcript shows only the user prompt, the collapsed process disclosure, and the final assistant answer.
- The full reasoning/tool history remains available on demand.

## 2. Current Baseline and Known Gaps

The existing frontend already wires most required behavior:

- session creation, restore, deletion, pagination, and multi-session state;
- streaming reasoning and answers;
- tool previews, final tool results, and exec progress;
- permission, ask-user, Plan Review, Tasks, Skills, Git, context, compact, undo, cancel, files, settings, themes, and localization.

Known prerequisites discovered during design:

- `cmd_permission_response` is defined and called by the frontend but is missing from the Tauri `generate_handler!` registration.
- `SettingsView.tsx` currently prevents `pnpm run build` because a `section` element is not closed.
- `round_complete.is_final` exists in the protocol but is not retained by the current chat store.
- `exec_progress.seq` is received but ignored.
- `code_delta` and several dashboard fields are not consumed by the UI.
- `tool_notice` is accepted but not rendered.
- `recentEdits` is stored and passed to `StatusPanel` but not displayed.
- the event entry point uses `Record<string, unknown>` instead of exhaustively handling generated `Agent2Ui` variants.

The existing Vitest baseline observed during design was 5 passing files and 11 passing tests. The frontend build was blocked by the `SettingsView.tsx` syntax error above.

## 3. Design Principles

1. Protocol facts remain complete; presentation becomes selective.
2. Final answers are the primary content. Process details are secondary but recoverable.
3. Tool calls are events in a turn, not standalone messages or persistent cards.
4. Only unresolved interactions may temporarily interrupt the transcript hierarchy.
5. Components consume presentation models, never raw protocol rounds.
6. Completed history restores in a collapsed state.
7. Failures remain visible and actionable.
8. Risk styling comes from backend risk classification, not frontend guesses.
9. Empty space is preserved for reading; engineering telemetry does not fill it by default.
10. The redesign must preserve all currently wired functionality.

## 4. Architecture

### 4.1 State layers

The frontend is split into three state layers:

```text
Tauri Agent2Ui events
        |
        v
SessionEventReducer
        |
        v
RawSessionState
        |
        v
TurnProjection + ProcessAggregation
        |
        v
ConversationViewModel
        |
        v
New UI components
```

`RawSessionState` stores protocol facts only:

```ts
type RawTurn = {
  turnId: string;
  userText: string;
  rounds: RawRound[];
  status: "running" | "waiting" | "completed" | "failed" | "cancelled";
  startedAt?: number;
  finishedAt?: number;
};

type RawRound = {
  roundNum: number;
  isFinal: boolean;
  blocks: RoundBlock[];
  toolCalls: ToolCallDef[];
  toolResults: Record<string, ToolResultDef>;
  progress: Record<string, OrderedProgress>;
};
```

`ConversationViewModel` stores projected presentation data:

```ts
type TurnViewModel = {
  userPrompt: UserPromptModel;
  process: ProcessTraceModel;
  finalAnswer?: FinalAnswerModel;
  interaction?: PendingInteractionModel;
};
```

Ephemeral UI state stores disclosure and shell behavior:

```text
expandedProcessByTurn
expandedProcessItemByTurn
environmentPopoverOpen
sidebarCollapsed
composerDraftBySession
followUpQueueBySession
```

Ephemeral state is not persisted in backend session history.

### 4.2 Event reducer

Move event handling out of `App.tsx` into a typed reducer. The reducer uses the generated `Agent2Ui` discriminated union and an exhaustive switch. Adding a protocol variant must cause a TypeScript compile failure until handled.

Required explicit handling includes:

- retain `round_complete.is_final`;
- apply `code_delta` to environment change totals;
- convert `tool_notice` into process items;
- order exec progress by `seq`;
- identify `ready`, `pong`, and `shutdown_ack` as lifecycle events;
- remove `tool_exec_delta` from the protocol if the backend no longer emits it.

### 4.3 Projection rules

Final answer selection:

1. Prefer ordered text blocks from a round with `is_final=true`.
2. Reasoning, tools, and text from non-final rounds belong to the process trace.
3. For legacy history without `is_final`, use the final non-empty answer as the final answer.
4. A successful tool-only turn displays a small completion statement instead of inventing assistant content.
5. Failed and cancelled turns do not fabricate a final answer.

Projection and aggregation are pure functions. Restored history and equivalent live events must produce the same view model.

## 5. Turn and Process Trace

### 5.1 Top-level completed turn

The top-level DOM for a normally completed turn contains only:

```text
UserPromptBubble
ProcessDisclosure (collapsed by default)
AssistantAnswer
```

There are no visible tool cards, reasoning cards, avatars, role labels, or round wrappers.

### 5.2 Disclosure lifecycle

```text
running   -> "Processing 18s"       -> expanded
waiting   -> "Needs your approval"  -> expanded
completed -> "Processed in 4m 9s"   -> forcibly collapsed on turn_end
failed    -> "Processing failed"    -> expanded
cancelled -> "Stopped"              -> expanded
```

Users may reopen a completed process trace. Restored completed turns start collapsed.

### 5.3 Process item types

```ts
type ProcessItem =
  | ReasoningPhase
  | AssistantProgressNote
  | ToolActivity
  | ActivityGroup
  | InteractionRecord
  | SystemNotice;
```

Only one process item detail is expanded per turn by default.

### 5.4 Aggregation

- Consecutive reasoning deltas merge into a reasoning phase.
- Consecutive successful reads, listings, and searches merge into a file activity group.
- Repeated writes to one file merge into one file row with cumulative diff statistics.
- Consecutive successful commands may merge into `Ran N commands`.
- A failed command is never hidden inside a successful aggregate.
- Similar Web and Skill operations may merge into compact semantic groups.
- Non-final assistant text becomes an assistant progress note inside the trace.
- Resolved permissions, ask-user questions, and plan approvals become historical interaction rows.

### 5.5 Detail rendering

Rows remain one line until expanded. Expanded details may render commands, output, file lists, diffs, errors, or received reasoning content.

Large output renders a bounded preview first. Full content mounts only after explicit user action. Existing Markdown, syntax highlighting, ANSI conversion, and diff parsing may be retained as parsing utilities, but not with the old ToolCard DOM.

### 5.6 Stable streaming

Each tool call uses `tool_call_id` as a stable identity. Streaming updates cannot remount the item or reset disclosure state.

Exec buffering is maintained per tool call and stream:

```ts
Map<toolCallId, {
  stdout: Map<seq, chunk>;
  stderr: Map<seq, chunk>;
  nextExpectedSeq: number;
}>
```

Final tool results close the buffer but preserve the current UI expansion state. Missing sequence numbers produce a non-blocking incomplete-output notice.

## 6. Interactive Gates

Permission prompts, ask-user questions, and Plan Review actions are not ordinary tool events.

- While unresolved, they are promoted beneath the active process trace and become the current focus.
- When resolved, they collapse into one process history row.
- Failed permission responses remain in place with a retry action.
- Normal composer follow-ups do not bypass an unresolved gate.

### 6.1 Permission risk

The permission request protocol adds:

```ts
risk: "low" | "medium" | "high";
consequence?: string;
```

Risk is computed by the Rust permission layer after resource normalization. The current permission policy level is not reused as an action-risk score.

Visual mapping:

- low-risk approval: normal dark button;
- medium-risk approval: red outline button;
- high-risk approval: red solid button;
- rejection: neutral button, avoiding two competing red actions.

High-risk prompts show the resource scope and consequence. Button text states the grant scope, such as `Allow once` or `Allow and trust this folder`.

## 7. App Shell

### 7.1 Component tree

```text
AppShell
|- TaskSidebar
|- ThreadWorkspace
|  |- ThreadHeader
|  |- ConversationTranscript
|  |  `- TurnGroup
|  |     |- UserPromptBubble
|  |     |- ProcessDisclosure
|  |     |  `- ProcessTimeline
|  |     `- AssistantAnswer
|  `- ComposerDock
|- EnvironmentPopover
`- InteractionLayer
   |- PermissionPrompt
   |- AskUserPrompt
   `- PlanApprovalPrompt
```

### 7.2 Task sidebar

Sessions become tasks and the sidebar is the sole visible session switcher. The current top open-tabs strip is removed.

The sidebar contains only wired destinations:

- New task;
- Skills;
- Settings;
- task history.

Unsupported Codex destinations such as Projects, Scheduled Tasks, Plugins, and Pull Requests are not faked. Session titles prefer session title, last summary, first prompt, then a short seed. Delete and secondary actions live in a row context menu.

Workspace controls move out of the sidebar.

### 7.3 Thread header

The header shows task title and compact contextual actions. Model, seed, context, and cache telemetry are removed from the permanent header.

The overflow menu contains only wired actions such as undo, compact, delete, and close. `Open location` and `Environment` remain direct controls.

### 7.4 Environment popover

The permanent `StatusPanel` is removed. Environment information appears in an overlay that does not reserve content width.

The popover contains:

- initial and incremental Git change totals;
- workspace;
- branch;
- context usage;
- model;
- commit, branch, and diff actions.

Data sources are existing Git/workspace/session commands plus the newly consumed `code_delta` event.

Old StatusPanel content moves as follows:

- Git and context -> EnvironmentPopover;
- Skills -> SkillsView;
- Tasks -> in-conversation plan/task presentation;
- Activity -> ProcessDisclosure;
- Plan Review -> in-conversation interaction;
- context charts -> secondary environment detail.

### 7.5 Composer

The composer is a centered floating dock aligned to the transcript column. It retains multiline text, attachments, Plan/Code mode, permission level, model indication, slash commands, Skills, send/stop, and error-restored text.

During a running turn, the text area remains editable. Submitted follow-ups enter a per-session local queue:

1. queued text is not inserted into the current turn;
2. the composer shows the queue count;
3. users may edit or remove queued entries;
4. after `turn_end`, the first item sends only if no gate remains unresolved;
5. later entries send one per completed turn;
6. application shutdown does not automatically execute queued work;
7. after cancellation, the UI asks whether queued work should continue.

### 7.6 Other pages

The existing Home dashboard is removed. With no selected task, the thread workspace shows a lightweight new-task entry and recent tasks.

Token usage moves to a Settings usage section. Settings becomes a categorized settings page. Skills becomes a dedicated view for project/user scopes, source paths, active state, activation, unloading, and refresh.

### 7.7 Native window

The complete redesign includes a custom Tauri titlebar with Windows drag, minimize, maximize, close, DPI, and maximized-state behavior. This requires Tauri configuration and Rust/window integration; it is not a frontend CSS-only task.

### 7.8 Responsive rules

- at 1100px and above: full task sidebar and environment popover;
- from 800px to 1099px: narrower sidebar; environment overlays content without resizing it;
- below 800px: sidebar becomes a drawer and environment becomes a bottom sheet;
- the composer and transcript share one width token;
- the composer is capped near 760px and preserves 12px side margins on narrow windows.

## 8. Visual Language

- Near-white canvas, quiet gray surfaces, subtle borders, and sparse status color.
- DeepX orange remains a restrained brand accent, not the default color for every action or running state.
- User prompts use a compact light-gray bubble aligned to the right.
- Assistant answers use flat document typography without an avatar or assistant label.
- Code blocks use a quiet header with language/file and copy action.
- Diffs use semantic add/remove colors only inside diff content and statistics.
- Completed process disclosure is visually subordinate to the answer.
- One orchestrated collapse transition is preferred over per-tool exit animations.
- Reduced-motion preferences disable height and reveal animation.

## 9. Error and Recovery Behavior

- Unknown protocol events create a protocol diagnostic and non-blocking toast.
- Projection failure degrades the affected turn to a raw text summary instead of blanking the transcript.
- Tool failure leaves the process trace expanded.
- Agent disconnect preserves the current trace and composer draft; reconnect deduplicates by turn ID.
- Permission response failure leaves the prompt visible and retryable.
- Environment fetch failure marks only that row unavailable.
- Markdown failure falls back to plain text.
- A successful turn with no final text shows a small completion statement.

## 10. Replacement and Deletion

Replacement targets include:

```text
MessageItem.tsx
ThinkingBlock.tsx
ToolRow.tsx
MessageList.tsx
InfoBar.tsx
StatusPanel.tsx
InputBar.tsx
open-tabs UI
Home Token Dashboard
legacy message/tool/status/info/input CSS
```

Do not recreate these old ideas under new names:

- one card per tool;
- one top-level answer per round;
- reasoning at the same hierarchy as the final answer;
- permanent status rail;
- permanent engineering telemetry bar;
- avatars and role labels;
- success decoration on every tool;
- orange primary styling on every action;
- components parsing raw protocol fields;
- chunk updates rebuilding a complete turn.

## 11. Implementation Sequence

### Phase 0: restore a reliable baseline

- fix the Settings syntax error;
- register `cmd_permission_response`;
- establish current frontend and Rust checks;
- add exhaustive Agent2Ui checking;
- settle the obsolete `tool_exec_delta` variant.

### Phase 1: protocol and presentation foundation

- retain `is_final`;
- add permission risk metadata;
- consume missing events;
- implement the session event reducer;
- implement turn projection and process aggregation;
- implement ordered exec progress;
- add live/restore projection fixtures.

### Phase 2: conversation content

- build the new transcript, turn, process, answer, and interaction components;
- migrate Markdown, code, ANSI, and diff parsing utilities;
- delete old message and tool components after parity.

### Phase 3: shell

- build sidebar, header, environment popover, composer, follow-up queue, and empty workspace;
- delete open tabs, permanent status/info bars, and Home dashboard.

### Phase 4: special views and window shell

- redesign Skills and Settings;
- complete Git/context detail;
- integrate custom Tauri titlebar;
- finish theme and responsive behavior.

### Phase 5: cleanup and validation

- remove the temporary comparison flag;
- remove legacy references and selectors;
- verify bundle and console output;
- complete functional and visual regression.

A temporary `deepx:new-conversation-ui` flag may be used during development only. It must be removed before final integration; two UI paths are not maintained long term.

## 12. Crate and Commit Boundaries

Keep protocol prerequisites separate from the UI replacement:

1. `deepx-proto`: final-round and permission-risk DTO contract;
2. Rust risk producer: normalized action-risk output;
3. `deepx-tauri/src-tauri`: command registration, forwarding, and native window shell;
4. `deepx-tauri/src`: reducer, projection, and visual replacement.

The primary UI work remains scoped to `deepx-tauri`. Cross-crate protocol work lands first and does not mix unrelated refactors.

## 13. Acceptance Criteria

### Functional

The redesign preserves session lifecycle, history paging, streaming, tool progress/results, stdout/stderr, permission, ask-user, Plan Review, Skills, Tasks, Git, compact, undo, cancel, files, settings, themes, localization, reconnect, and multi-session isolation.

### Transcript

- completed turns show only prompt, collapsed process disclosure, and final answer at top level;
- running turns show one expanded process disclosure;
- successful operations aggregate;
- failures remain visible;
- streaming updates preserve expansion state;
- follow-up input queues during a running turn.

### Permission

- risk comes from the backend;
- only high-risk approval is red solid;
- medium-risk approval is red outline;
- low-risk approval is normally styled;
- high-risk prompts show scope and consequence;
- failed responses remain retryable.

### Shell

- no permanent right rail;
- transcript and composer share an axis;
- environment overlays without resizing the transcript;
- sidebar is the sole task/session switcher;
- no fake unsupported navigation.

### Engineering

- all Agent2Ui variants are explicitly handled;
- projection and aggregation are pure and tested;
- restore and live paths produce equivalent models;
- stable IDs prevent streaming remounts;
- large output is mounted lazily;
- legacy components and CSS are removed;
- no protocol escape through untyped `any` in the event path;
- keyboard focus, screen-reader labels, and reduced motion are supported.

Validation gates:

```text
pnpm run build
pnpm run test:run
cargo check -p deepx-tauri
cargo test -p deepx-tauri
```

Visual regression covers light and dark themes at 1920x1080, 1366x768, and a narrow desktop window.

## 14. Non-Goals

- copying Codex branding, trademarked assets, or proprietary icons;
- inventing unsupported Projects, Scheduled Tasks, Plugins, or Pull Request features;
- rewriting the agent reasoning or tool execution kernel;
- persisting disclosure state to backend history;
- adding multi-user collaboration or cloud sync;
- permanently maintaining old and new UI paths.

## 15. Final Boundary

Protocol facts remain complete and auditable. The visual layer is rebuilt. Running work remains observable, but completed work retreats into one disclosure so that the user prompt and final assistant answer always dominate the transcript.
