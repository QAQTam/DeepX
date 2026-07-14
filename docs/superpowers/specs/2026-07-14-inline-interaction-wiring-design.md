# DeepX Inline Interaction Wiring

## Goal

Render ask-user, permission, and context-compaction interactions in one low-noise dock immediately above the active chat composer. Preserve the existing Tauri commands and stores; remove the old ask overlays from the production render path.

## Ownership

- `App` retains the global permission queue and the `cmd_permission_response` callback.
- `ChatView` owns placement of `InteractionDock` because it owns the active transcript and composer.
- The active chat store remains authoritative for ask-user and compaction state.
- `ComposerDock` remains responsible only for message input and its pending-gate disabled state.

## Data flow

1. `App` passes the permission queue item and response callback to `ChatView` only when the queued permission belongs to the active session.
2. `ChatView` renders one `InteractionDock` immediately before `ComposerDock` when ask-user, permission, or compaction UI is present.
3. `AskUserPrompt` submits through the existing `chat.submitAskAnswer` and dismisses through `chat.dismissAsk`.
4. `PermissionPrompt` responds through the callback supplied by `App`, which invokes `cmd_permission_response`, resolves reducer state, and advances the queue.
5. `CompactStatusRow` reads `isCompacting`, `compactText`, and `compactResult`; it does not invoke compaction itself.

## Rendering rules

- Permission takes precedence over ask-user if both are present; compaction status may remain visible above either prompt.
- No interaction uses a fullscreen or centered overlay.
- High-risk approval stays a solid red action; rejection stays neutral.
- The transcript remains visible and the composer remains directly below the dock.
- Existing `hasPendingGate` behavior continues to disable message submission while permission is pending.

## Error and session behavior

- Failed permission responses leave the queue item visible so the user can retry.
- A permission for another session is not shown in the active chat; switching to its session reveals it.
- Existing event and protocol DTOs are reused without modification.

## Verification

- Component/integration tests assert dock placement, old ask-overlay removal, ask submission, permission response, high-risk styling, and compact state rendering.
- Run the full frontend test suite, TypeScript/Vite build, and `git diff --check`.

## Non-goals

- No Rust or protocol changes.
- No redesign of permission classification or compact lifecycle.
- No deletion of legacy component files in this pass; only remove them from production rendering.
