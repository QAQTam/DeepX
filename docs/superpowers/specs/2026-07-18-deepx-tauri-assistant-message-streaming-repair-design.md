# DeepX Tauri Assistant Message and Streaming Transcript Repair Design

**Date:** 2026-07-18

**Status:** Approved design; awaiting review of this written specification

## Context

The Tauri transcript currently loses the protocol-level distinction between assistant text and tool/process content. `RawRound.blocks` stores ordered `RoundBlock` values, but `turnProjection` ignores them and derives one `round.answer` plus a process timeline. As a result, an assistant text block emitted in a tool-call round may be folded into or disappear behind the process UI.

The current Markdown terminal transition also writes the complete Markdown source to the DOM before asynchronous Shiki highlighting completes. This visibly presents the raw source and then the rendered result. The process disclosure receives `defaultOpen` from `TurnGroup`, which disables its completion-close behavior. Finally, transcript auto-scroll observes only turn count and final turn identity, so same-turn answer deltas, tool-output deltas, and Markdown height changes do not follow the latest output.

## Goals

1. Render every assistant text block as an independent visible assistant chat item, including text in tool-call rounds.
2. Keep assistant chat history; never delete earlier assistant text simply because a final answer arrives.
3. Keep reasoning and tools inside process disclosures, initially collapsed for tool-call work.
4. Collapse tool process disclosures when the turn reaches a terminal lifecycle event, without collapsing assistant chats.
5. Replace terminal Markdown atomically, without exposing the complete raw source alongside Markdown-rendered output.
6. Follow live output while the reader remains at the transcript bottom, without overriding an intentional scroll-away.
7. Preserve compatibility with replayed/older round payloads that only have `answer` and lack ordered `blocks`.

## Non-goals

- No protocol change and no generated binding edits.
- No redesign of the transcript shell, tool detail UI, Markdown styling, or session/replay architecture.
- No deletion of historical assistant messages after `done` or `turn_end`.
- No forced scrolling while the user is reading older content.

## Confirmed Root Causes

| Symptom | Cause | Evidence |
| --- | --- | --- |
| Assistant chat in a tool round is folded or missing | The reducer stores `RawRound.blocks`, but `projectTurn` ignores them and derives only one `answer` plus a process item list. | `crates/deepx-tauri/src/store/sessionEventReducer.ts:231-239`; `crates/deepx-tauri/src/presentation/turnProjection.ts:35-66` |
| Full raw Markdown appears before final Markdown | Final render synchronously assigns `container.textContent = text` before waiting for Shiki and then replaces it with HTML. | `crates/deepx-tauri/src/components/MarkdownBody.tsx:251-272` |
| Tool rounds do not close automatically | `TurnGroup` always passes `defaultOpen`; `ProcessDisclosure` bypasses auto-close whenever that property exists. | `crates/deepx-tauri/src/components/conversation/TurnGroup.tsx:24-36`; `crates/deepx-tauri/src/components/process/ProcessDisclosure.tsx:21-23` |
| Live output does not keep viewport at the bottom | The scroll effect responds only to turn-array length or last turn ID, neither of which changes for same-turn stream deltas or post-render height growth. | `crates/deepx-tauri/src/components/conversation/ConversationTranscript.tsx:33-43` |

## Architecture

### Ordered round presentation

`RoundBlock` is the authoritative presentation source when it is non-empty.

A round is projected into ordered render entries:

- `RoundBlock::Text` becomes one independent assistant-message entry.
- Adjacent `RoundBlock::Reasoning` and `RoundBlock::Tool` values become one process entry, preserving their position between assistant entries.
- A process entry contains only reasoning and tools. It never owns assistant text.
- Multiple interleaved blocks preserve order. For example, `text -> tool -> text` projects to `assistant chat -> collapsed process -> assistant chat`.

When `blocks` is absent or empty, the projection uses existing fields as a compatibility fallback: reasoning and tool calls build the process entry, and non-empty `answer` builds exactly one assistant entry. It must not render both fallback `answer` and a `Text` block containing the same content.

During `round_delta(kind = answering)`, the active round displays one transient assistant entry after its current process entry. `round_complete` with non-empty blocks replaces that transient presentation with the block-derived entries. This prevents duplicate text once the authoritative event arrives.

### Component boundaries

`turnProjection` owns the conversion from raw protocol state to explicitly typed ordered entries. `TurnGroup` renders those entries in sequence. `AssistantAnswer` remains responsible only for an independent Markdown-backed chat item. `ProcessDisclosure` remains responsible only for one process entry and its expanded state.

This keeps event data, view projection, row rendering, Markdown rendering, and scroll behavior separate. Existing `RawRound.blocks` remains the sole persisted/replayable ordered model; no parallel conversation model is introduced.

### Process disclosure behavior

All process entries initialize collapsed, including reasoning-only entries. A user-triggered expansion stays stable while that process entry receives new data. Terminal status (`completed`, `failed`, or `cancelled`) auto-collapses the process disclosure. `done` already converts active raw turns to `completed`; normal `turn_end` sets the same terminal state, so the UI consumes status rather than adding a second terminal event channel.

Assistant chat entries are sibling rows, outside `ProcessDisclosure`, and are unaffected by all disclosure state transitions.

## Markdown Terminal Rendering

Markdown body rendering retains the existing latest-generation cancellation guard.

When an answer moves from streaming to final:

1. Preserve the currently displayed streaming DOM while parsing/highlighting is pending.
2. Render final Markdown HTML asynchronously.
3. If the generation remains current, replace the existing content in one DOM operation.
4. If Shiki initialization or highlighting fails, synchronously render plain Markdown HTML through `marked` without the Shiki renderer, then perform the same replacement.

The final path must never set the complete raw Markdown string as a temporary `textContent` value. Partial streaming tail text may remain raw while the stream is active; the defect concerns the final transition only.

## Follow-Tail Scrolling

`ConversationTranscript` owns a follow-tail signal:

- It begins enabled on initial render and after explicitly choosing the jump-to-bottom control.
- A user scroll that leaves a defined bottom threshold disables it and exposes the jump control.
- While enabled, content mutation and transcript-size changes schedule one scroll-to-bottom per animation frame.
- A `ResizeObserver` on the transcript content detects answer growth and post-async Markdown height changes. It must request follow-tail scrolling only when follow-tail remains enabled.
- Prepending older turns preserves current viewport distance and must not re-enable follow-tail.
- Programmatic follow-tail scrolling must not be interpreted as user intent to disable following.

This avoids per-token synchronous scroll thrashing while maintaining visibility of the active stream.

## Error Handling and Compatibility

- Empty or malformed optional block content is ignored in the same way as existing empty answer/thinking data.
- A missing `blocks` field has the defined answer-based fallback, including restored historical sessions.
- Markdown highlighter failure does not erase visible content and does not expose a full raw final source; plain Markdown rendering is the deterministic fallback.
- `ResizeObserver` is cleaned up on component disposal. Environments without it use the existing mutation-triggered scheduling path without throwing.
- The feature does not alter reducer event idempotency, replay ordering, or session state ownership.

## Tests and Acceptance Criteria

### Projection and transcript tests

- A round containing `text -> tool -> text` produces three ordered visible entries and both assistant messages remain outside a process disclosure.
- An answer-only historical round produces one assistant entry.
- A round with blocks and a matching `answer` renders assistant text once.
- An answering delta creates one transient assistant entry that is replaced, not duplicated, by authoritative block content.

### Disclosure tests

- A tool process begins collapsed.
- A user expansion remains open during live updates.
- Transitioning the owning turn to each terminal status closes the process disclosure.
- Assistant chat items remain visible before and after terminal closure.

### Markdown tests

- Transitioning to final output never exposes the full Markdown source while waiting for syntax highlighting.
- A stale asynchronous markdown render cannot overwrite newer content.
- A highlighter failure produces Markdown-rendered fallback output.

### Scroll tests

- New same-turn assistant text and tool output scroll to the bottom while follow-tail is enabled.
- A simulated transcript height change after async Markdown rendering also scrolls to the bottom while follow-tail is enabled.
- User scroll-away disables following and displays the jump control.
- Jump-to-bottom reenables following.
- Older-turn prepend preserves viewport distance and leaves following disabled unless it was already enabled by user action.

### Verification gates

Run the focused Vitest cases, complete frontend test suite and build, then Rust checks required by the existing project workflow:

```powershell
pnpm --dir crates/deepx-tauri test:run
pnpm --dir crates/deepx-tauri build
cargo check -p deepx-tauri
cargo test -p deepx-tauri
git diff --check
```

The current environment did not resolve `pnpm` or `corepack`; before implementation, locate an installed Node package runner or explicitly report the unavailable frontend gate. This environmental issue does not change the implementation design.

Manual smoke testing must cover a response with assistant text before tools, between tools, and final assistant text; user expansion while a tool executes; terminal completion; Markdown code block finalization; and scroll-away during streaming followed by the jump-to-bottom action.
