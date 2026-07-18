# Tauri Assistant Messages and Streaming Transcript Repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use subagent-driven-development (recommended) or executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render protocol assistant-text blocks as independent chats, keep tool process panels collapsed and terminally closed, remove final Markdown raw-source flashes, and follow the active stream unless the reader intentionally scrolls away.

**Architecture:** Keep `RawRound.blocks` as the sole ordered protocol model. Extend the presentation projection with ordered assistant/process entries and let `TurnGroup` render those entries as sibling rows. Keep Markdown rendering and transcript follow-tail behavior self-contained in their existing components; no protocol, generated binding, reducer ownership, or session lifecycle changes are required.

**Tech Stack:** TypeScript 6, SolidJS 1.9, Vitest 4 with jsdom, `marked`, Shiki, Tauri frontend, Rust workspace verification.

## Global Constraints

- Work directly in the current repository; do not create a worktree.
- Do not modify `deepx-proto`, generated TypeScript bindings, backend event emission, or the session reducer event contract.
- `RawRound.blocks` is authoritative when non-empty; `answer` is only the legacy/replay fallback when `blocks` is empty.
- A `RoundBlock::Text` is always a visible assistant chat sibling, never content inside a tool disclosure.
- Process panels initialize collapsed, preserve a user expansion during live updates, and close on `completed`, `failed`, or `cancelled` status.
- Preserve historical assistant chat messages after `done` and `turn_end`.
- Do not force-scroll after the user leaves the transcript bottom threshold; the jump control restores following.
- The terminal Markdown path must not temporarily place the complete raw Markdown source into the DOM.
- Do not touch pre-existing user changes outside the listed frontend files.

---

## File Structure

| File | Responsibility |
| --- | --- |
| `crates/deepx-tauri/src/presentation/turnProjection.ts` | Convert `RawRound` ordered blocks into ordered assistant/process presentation entries, with legacy fallback. |
| `crates/deepx-tauri/src/presentation/turnProjection.test.ts` | Verify ordered blocks, fallback behavior, and delta/final de-duplication at projection level. |
| `crates/deepx-tauri/src/components/conversation/TurnGroup.tsx` | Render each projected assistant/process entry in order as independent sibling UI. |
| `crates/deepx-tauri/src/components/conversation/TurnGroup.test.tsx` | Verify assistant rows remain outside process panels and render in protocol order. |
| `crates/deepx-tauri/src/components/process/ProcessDisclosure.tsx` | Own disclosure initialization, user toggles, and terminal auto-close. |
| `crates/deepx-tauri/src/components/process/ProcessDisclosure.test.tsx` | Verify default collapse, live user expansion, and terminal auto-close. |
| `crates/deepx-tauri/src/components/MarkdownBody.tsx` | Atomically replace streaming content with final Markdown, including no-Shiki fallback. |
| `crates/deepx-tauri/src/components/MarkdownBody.test.tsx` | Verify raw final source does not flash, latest render wins, and highlighter failure falls back to Markdown. |
| `crates/deepx-tauri/src/components/conversation/ConversationTranscript.tsx` | Track reader follow-tail intent and schedule scrolling for stream and height changes. |
| `crates/deepx-tauri/src/components/conversation/ConversationTranscript.test.tsx` | Verify follow-tail, scroll-away, jump-to-bottom, resize behavior, and prepended-history preservation. |

## Environment Preparation

The previous direct executable lookup for `pnpm` failed even though PowerShell resolves `C:\Users\tsy3m\AppData\Roaming\npm\pnpm.ps1` and `crates/deepx-tauri/node_modules` exists. Run all frontend gates through PowerShell so its profile command resolution invokes that script.

### Task 1: Establish the frontend test runner and baseline

**Files:**
- Modify: none

**Interfaces:**
- Consumes: existing `crates/deepx-tauri/package.json` scripts and local `node_modules`.
- Produces: a repeatable PowerShell command form used by every following task.

- [ ] **Step 1: Resolve the PowerShell pnpm command**

Run:

```powershell
pwsh -Command "(Get-Command pnpm -ErrorAction Stop).Source; pnpm --version"
```

Expected: prints `C:\Users\tsy3m\AppData\Roaming\npm\pnpm.ps1` and a pnpm version.

- [ ] **Step 2: Run the focused baseline suite**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/presentation/turnProjection.test.ts src/components/conversation/TurnGroup.test.tsx src/components/process/ProcessDisclosure.test.tsx src/components/MarkdownBody.test.tsx src/components/conversation/ConversationTranscript.test.tsx"
```

Expected: PASS before edits. If this fails because dependencies are unavailable, record the exact command output in the implementation handoff and do not replace the configured package manager or lockfile.

- [ ] **Step 3: Commit**

No code change is expected. Do not create an empty commit.

### Task 2: Project protocol blocks into ordered assistant/process entries

**Files:**
- Modify: `crates/deepx-tauri/src/presentation/turnProjection.ts`
- Modify: `crates/deepx-tauri/src/presentation/turnProjection.test.ts`

**Interfaces:**
- Consumes: `RawRound.blocks`, `RawRound.thinking`, `RawRound.answer`, `RawRound.toolCalls`, `RawRound.toolResults`, and `RawRound.progress`.
- Produces: `RoundRenderEntry`, `RoundViewModel.entries`, and `projectTurn(rawTurn): TurnViewModel`.
- Consumers: `TurnGroup` in Task 3.

- [ ] **Step 1: Write failing projection tests for ordered blocks and legacy fallback**

Replace the old `answer`/`processItems`-only assertions with tests that create one round whose blocks are text, tool, then text. Assert exactly three entries, their kinds, and their content/order. Add a second test with `blocks: []`, `answer: "legacy answer"`, and one tool call; assert one process entry and one assistant entry. Add a third test with both `blocks: [{ type: "text", content: "authoritative" }]` and `answer: "stale fallback"`; assert exactly one assistant entry with `"authoritative"`.

```ts
expect(view.rounds[0].entries.map(entry => entry.kind))
  .toEqual(["assistant", "process", "assistant"]);
expect(view.rounds[0].entries[0]).toMatchObject({
  kind: "assistant", markdown: "before tool",
});
expect(view.rounds[0].entries[1]).toMatchObject({
  kind: "process", hasTools: true,
});
expect(view.rounds[0].entries[2]).toMatchObject({
  kind: "assistant", markdown: "after tool",
});
```

- [ ] **Step 2: Run the projection test to verify it fails**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/presentation/turnProjection.test.ts"
```

Expected: FAIL because `RoundViewModel.entries` does not exist.

- [ ] **Step 3: Define the ordered presentation types and block helpers**

In `turnProjection.ts`, replace the old round shape with explicit entries:

```ts
export type RoundRenderEntry =
  | { kind: "assistant"; id: string; markdown: string; streaming: boolean }
  | { kind: "process"; id: string; items: ProcessItem[]; hasTools: boolean };

export type RoundViewModel = {
  roundNum: number;
  isFinal: boolean;
  entries: RoundRenderEntry[];
};
```

Extract helpers that map a reasoning string and a `ToolCallDef` to the current `ProcessItem` representation. The tool helper must use the call ID to attach `toolResults[call.id]` and `progress[call.id]?.chunks`, exactly as the current projection does.

- [ ] **Step 4: Implement block-authoritative ordered projection**

Add `projectRoundEntries(rawTurn, round, isLiveRound)` with the following invariant:

```ts
if (round.blocks.length > 0) {
  // Iterate blocks left-to-right.
  // Buffer reasoning/tool ProcessItem values.
  // Flush one process entry immediately before each text block and at end.
  // Emit a text block as { kind: "assistant", markdown: content, streaming: false }.
  // Do not read round.answer in this branch.
} else {
  // Create at most one process entry from thinking and toolCalls.
  // Then emit one assistant entry only when round.answer.trim() is non-empty.
  // Its streaming flag is isLiveRound.
}
```

Use stable entry IDs containing `turnId`, `roundNum`, and the block/process ordinal, for example `"${turnId}-round-${roundNum}-assistant-${ordinal}"`. Pass each process buffer through `aggregateProcessItems` only after grouping contiguous protocol process blocks, so a text block always separates process entries.

- [ ] **Step 5: Update `projectTurn` to determine the live round once**

Compute the last round index while iterating `rawTurn.rounds`. Pass `rawTurn.status === "running" && index === lastRoundIndex` to `projectRoundEntries`. This makes the answer-only delta path transient, while an authoritative completed block sequence remains non-streaming. Preserve elapsed time, interactions, total tokens, and tokens-per-second logic unchanged.

- [ ] **Step 6: Run the projection test to verify it passes**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/presentation/turnProjection.test.ts"
```

Expected: PASS, including ordered `text -> tool -> text`, answer-only compatibility, and no block/answer duplicate.

- [ ] **Step 7: Commit**

```powershell
git add crates/deepx-tauri/src/presentation/turnProjection.ts crates/deepx-tauri/src/presentation/turnProjection.test.ts
git commit -m "fix(tauri): project assistant text blocks independently"
```

### Task 3: Render ordered assistant entries outside collapsed process panels

**Files:**
- Modify: `crates/deepx-tauri/src/components/conversation/TurnGroup.tsx`
- Modify: `crates/deepx-tauri/src/components/conversation/TurnGroup.test.tsx`

**Interfaces:**
- Consumes: `TurnViewModel.rounds[].entries` from Task 2 and `ProcessDisclosure` from Task 4.
- Produces: protocol-ordered assistant message and process panel sibling rows.
- Consumers: `ConversationTranscript` and transcript UI tests.

- [ ] **Step 1: Write failing component tests for interleaved entries**

Update the mocked view model to use `entries`. Add a turn with an assistant entry, a process entry containing a tool, and a second assistant entry. Assert two `[data-part="assistant-answer"]` elements, assert the process panel is present, and assert neither answer is a descendant of `[data-part="process"]`.

```tsx
expect(host.querySelectorAll('[data-part="assistant-answer"]')).toHaveLength(2);
expect(host.querySelector('[data-part="process"]')?.textContent)
  .not.toContain("before tool");
expect(host.querySelector('[data-part="process"]')?.textContent)
  .not.toContain("after tool");
```

Keep a running, answer-only entry test and assert its mocked Markdown child receives `data-markdown-final="false"`.

- [ ] **Step 2: Run the component test to verify it fails**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/components/conversation/TurnGroup.test.tsx"
```

Expected: FAIL because `TurnGroup` still reads `round.processItems` and `round.answer`.

- [ ] **Step 3: Render entries in order**

Replace the per-round `Show` blocks in `TurnGroup.tsx` with an `Index each={round().entries}`. Render entries as follows:

```tsx
<Index each={round().entries}>
  {(entry) => (
    <Show
      when={entry().kind === "assistant"}
      fallback={
        <div data-part="process">
          <ProcessDisclosure status={status()} defaultOpen={false}>
            <ProcessTimeline items={(entry() as Extract<RoundRenderEntry, { kind: "process" }>).items} />
          </ProcessDisclosure>
        </div>
      }
    >
      <AssistantAnswer
        markdown={(entry() as Extract<RoundRenderEntry, { kind: "assistant" }>).markdown}
        streaming={(entry() as Extract<RoundRenderEntry, { kind: "assistant" }>).streaming}
      />
    </Show>
  )}
</Index>
```

Import `RoundRenderEntry` as a type. Do not retain `stage` presentation or infer “final answer”; every assistant block is now an independent historical chat item.

- [ ] **Step 4: Run the component test to verify it passes**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/components/conversation/TurnGroup.test.tsx"
```

Expected: PASS; two assistant rows remain visible and tool content is confined to the process entry.

- [ ] **Step 5: Commit**

```powershell
git add crates/deepx-tauri/src/components/conversation/TurnGroup.tsx crates/deepx-tauri/src/components/conversation/TurnGroup.test.tsx
git commit -m "fix(tauri): render assistant chats outside tool panels"
```

### Task 4: Make process panels collapsed by default and terminally self-closing

**Files:**
- Modify: `crates/deepx-tauri/src/components/process/ProcessDisclosure.tsx`
- Modify: `crates/deepx-tauri/src/components/process/ProcessDisclosure.test.tsx`

**Interfaces:**
- Consumes: terminal `ProcessStatus` supplied by `TurnGroup`.
- Produces: a disclosure that initializes from `defaultOpen`, keeps user toggles during live updates, and closes on terminal status.
- Consumers: all process entries rendered by Task 3.

- [ ] **Step 1: Replace the contradictory disclosure tests**

Replace the test that expects `defaultOpen={true}` to defeat terminal closing. Add tests for:

```tsx
it("starts collapsed when defaultOpen is false", () => {
  // status="running", defaultOpen={false} => aria-expanded "false"
});

it("keeps a user expansion while status remains running", async () => {
  // Click trigger, update a parent signal while status stays "running",
  // and assert aria-expanded remains "true".
});

it.each(["completed", "failed", "cancelled"] as const)(
  "closes when status becomes %s", async (terminal) => {
    // start running and open; set terminal; expect aria-expanded "false".
  },
);
```

- [ ] **Step 2: Run the disclosure test to verify it fails**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/components/process/ProcessDisclosure.test.tsx"
```

Expected: FAIL because the current effect returns early whenever `defaultOpen` exists.

- [ ] **Step 3: Implement terminal close without resetting active user state**

Use a terminal predicate and retain the initialization-only signal:

```ts
const [open, setOpen] = createSignal(props.defaultOpen ?? false);
const isTerminal = () =>
  props.status === "completed" || props.status === "failed" || props.status === "cancelled";

createEffect(() => {
  if (isTerminal()) setOpen(false);
});
```

Do not set `open` in response to a nonterminal status. This means an initial collapsed process stays collapsed, a user-expanded running process stays expanded across content updates, and all reducer-derived terminal paths (`done`, `turn_end`, `cancelled`, error-derived failure) close it.

- [ ] **Step 4: Run the disclosure test to verify it passes**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/components/process/ProcessDisclosure.test.tsx"
```

Expected: PASS for initial collapse, stable live expansion, and all terminal states.

- [ ] **Step 5: Commit**

```powershell
git add crates/deepx-tauri/src/components/process/ProcessDisclosure.tsx crates/deepx-tauri/src/components/process/ProcessDisclosure.test.tsx
git commit -m "fix(tauri): collapse completed tool processes"
```

### Task 5: Atomically finalize Markdown with a rendered fallback

**Files:**
- Modify: `crates/deepx-tauri/src/components/MarkdownBody.tsx`
- Modify: `crates/deepx-tauri/src/components/MarkdownBody.test.tsx`

**Interfaces:**
- Consumes: `MarkdownBodyProps.content`, `MarkdownBodyProps.final`, current highlighter promise, and DOM render generation guard.
- Produces: a final DOM replacement containing rendered Markdown, with Shiki enhanced code HTML when available and `marked`-only output when unavailable.
- Consumers: every independent `AssistantAnswer` from Task 3.

- [ ] **Step 1: Write failing tests for no raw final flash and fallback rendering**

Modify the existing deferred-highlighter test so it starts with a streaming value, then sets a final value containing Markdown syntax such as `"**new answer**"`. Before resolving Shiki, assert the old streaming DOM remains and the final literal Markdown source is not present. Resolve Shiki and assert final rendered text appears.

Add a separate test that makes `createHighlighter` reject. Render final `"**fallback answer**"`, wait for completion, assert text content contains `fallback answer`, and assert the container contains a `<strong>` element. This proves fallback uses `marked`, rather than raw `textContent`.

- [ ] **Step 2: Run the Markdown test to verify it fails**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/components/MarkdownBody.test.tsx"
```

Expected: FAIL because the final path assigns `container.textContent = text` before awaiting Shiki and catches highlighter errors without rendering Markdown.

- [ ] **Step 3: Add a no-Shiki Markdown rendering helper**

Extract the shared `marked.parse` configuration into a helper accepting an optional renderer. Keep Shiki code rendering in `renderBlockHTML`; add a fallback that renders with `marked.parse(raw, { async: false, gfm: true, breaks: false })`, applies the existing inline-background cleanup, and returns a string. Do not add a sanitizer or new dependency in this focused repair.

- [ ] **Step 4: Replace the final branch with delayed atomic DOM replacement**

In the `if (final)` branch, remove both `container.textContent = text` and the early raw-source update. Await `getHi()`, use the Shiki renderer if successful, otherwise use the fallback helper. After each await/error path, check `isStale()` before writing. Replace the container only after final HTML is available:

```ts
if (isStale()) return;
blocks[0]!.html = html;
container.replaceChildren(createStableEl(blocks[0]!));
container.classList.add("final");
prevBlocks = blocks;
```

Preserve the current streaming branch behavior and latest-generation cancellation semantics. If `marked.parse` itself throws, leave the already-visible streaming DOM unchanged and only update `prevBlocks` when the generation is still current.

- [ ] **Step 5: Run the Markdown test to verify it passes**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/components/MarkdownBody.test.tsx"
```

Expected: PASS; final raw source never flashes, stale work cannot overwrite new content, and rejected Shiki initialization renders Markdown fallback.

- [ ] **Step 6: Commit**

```powershell
git add crates/deepx-tauri/src/components/MarkdownBody.tsx crates/deepx-tauri/src/components/MarkdownBody.test.tsx
git commit -m "fix(tauri): atomically finalize markdown output"
```

### Task 6: Follow same-turn stream and post-render size changes

**Files:**
- Modify: `crates/deepx-tauri/src/components/conversation/ConversationTranscript.tsx`
- Modify: `crates/deepx-tauri/src/components/conversation/ConversationTranscript.test.tsx`

**Interfaces:**
- Consumes: reactive `turns`, transcript DOM height, user `scroll` events, and optional browser `ResizeObserver`.
- Produces: follow-tail behavior with `scheduleScrollToBottom()`, `followTail()` state, and jump-to-bottom re-enable action.
- Consumers: `ChatView` without prop/API changes.

- [ ] **Step 1: Add a jsdom `ResizeObserver` test double and failing behavior tests**

At the test file top, install a minimal global mock retaining the callback and exposing `trigger()`:

```ts
class ResizeObserverMock {
  static instances: ResizeObserverMock[] = [];
  constructor(private readonly callback: ResizeObserverCallback) {
    ResizeObserverMock.instances.push(this);
  }
  observe() {}
  disconnect() {}
  trigger() { this.callback([], this as unknown as ResizeObserver); }
}
vi.stubGlobal("ResizeObserver", ResizeObserverMock);
```

Add these tests with a configurable `scrollHeight`, `clientHeight`, and `scrollTo` spy:

1. Updating a same-ID turn with longer answer content while at bottom scrolls to the new bottom.
2. Calling `ResizeObserverMock.instances[0].trigger()` after increasing `scrollHeight` scrolls while follow-tail is enabled.
3. Dispatching a scroll event that leaves more than 120px below disables following; a later turn update does not scroll and shows `.jump-to-bottom`.
4. Clicking `.jump-to-bottom` scrolls and subsequent same-turn update follows again.
5. Preserve the existing prepend-distance test and additionally assert it does not make a scroll-away state follow again.

Stub `requestAnimationFrame` to queue callbacks and expose a local `flushAnimationFrames()` helper, so each test makes the scheduled behavior deterministic.

- [ ] **Step 2: Run the transcript test to verify it fails**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/components/conversation/ConversationTranscript.test.tsx"
```

Expected: FAIL because the existing effect ignores same-turn content changes and no resize observer exists.

- [ ] **Step 3: Replace length/last-ID detection with follow-tail scheduling**

In `ConversationTranscript.tsx`:

1. Rename `nearBottom` to `followTail` and initialize it to `true`.
2. Add `let scrollFrame: number | undefined` and `scheduleScrollToBottom()` that does nothing when `followTail()` is false or a frame is already scheduled; it invokes `scrollToBottom()` in one `requestAnimationFrame` callback.
3. Make `measure()` compare remaining distance to the existing `120` threshold and set `followTail(remaining < 120)` only for user scroll events.
4. Add a `createEffect` that reads `props.turns` and calls `queueMicrotask(scheduleScrollToBottom)` whenever the turn projection changes and follow-tail is enabled. This reacts to same-turn answer/tool data because `projectSession` creates a new turns array after each reduced state commit.
5. Attach a `ResizeObserver` to `.conversation-transcript` in `onMount`; its callback calls `scheduleScrollToBottom()`. In `onCleanup`, cancel a pending frame and disconnect the observer. Guard construction with `typeof ResizeObserver !== "undefined"`.
6. Preserve `loadOlder()` distance restoration exactly, but remove the old `prevLen`/`prevLastId` logic. Older content must not toggle follow-tail.
7. Change jump button condition to `!followTail()` and handler to `setFollowTail(true); scheduleScrollToBottom();`.

Use `scroller.scrollTo({ top: scroller.scrollHeight })` as today; no smooth behavior is permitted for automatic follow-tail writes.

- [ ] **Step 4: Run the transcript test to verify it passes**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/components/conversation/ConversationTranscript.test.tsx"
```

Expected: PASS for same-turn updates, asynchronous height changes, deliberate scroll-away, jump re-enable, and prepend preservation.

- [ ] **Step 5: Commit**

```powershell
git add crates/deepx-tauri/src/components/conversation/ConversationTranscript.tsx crates/deepx-tauri/src/components/conversation/ConversationTranscript.test.tsx
git commit -m "fix(tauri): follow live transcript output"
```

### Task 7: Run integration-focused frontend and Rust verification

**Files:**
- Modify: none unless a failing test exposes a defect in Tasks 2-6.

**Interfaces:**
- Consumes: all repaired presentation, disclosure, Markdown, and transcript code.
- Produces: evidence that the focused repair builds and does not violate repository diff checks.

- [ ] **Step 1: Run all focused regression tests together**

Run:

```powershell
pwsh -Command "Set-Location crates/deepx-tauri; pnpm exec vitest run src/presentation/turnProjection.test.ts src/components/conversation/TurnGroup.test.tsx src/components/process/ProcessDisclosure.test.tsx src/components/MarkdownBody.test.tsx src/components/conversation/ConversationTranscript.test.tsx"
```

Expected: PASS.

- [ ] **Step 2: Run complete frontend tests and type/build gate**

Run:

```powershell
pwsh -Command "pnpm --dir crates/deepx-tauri test:run"
pwsh -Command "pnpm --dir crates/deepx-tauri build"
```

Expected: both commands exit `0`. The build must type-check all changed `RoundViewModel` consumers; fix only failures caused by the planned interface migration.

- [ ] **Step 3: Run required Rust verification**

Run:

```powershell
cargo check -p deepx-tauri
cargo test -p deepx-tauri
git diff --check
```

Expected: every command exits `0`. Do not alter unrelated pre-existing Rust changes to make these gates pass.

- [ ] **Step 4: Perform the manual Tauri smoke pass**

In the running Tauri app, send a prompt that produces text before a tool call, text after a tool call, and a final answer. Verify:

1. Every assistant text is a separate visible chat item in protocol order.
2. Tool process is initially collapsed; manual expansion survives incoming output.
3. Process closes on turn completion while all assistant chats remain visible.
4. A completed Markdown code block changes directly from the active stream to rendered Markdown without a full raw-source flash.
5. While staying at bottom, streaming text and tool output remain visible; scroll upward, confirm automatic following stops and the jump button restores it.

- [ ] **Step 5: Commit verification fixes only if needed**

If an implementation correction was required by Steps 1-3, stage only the changed planned frontend files and create a focused commit:

```powershell
git add crates/deepx-tauri/src/presentation/turnProjection.ts crates/deepx-tauri/src/presentation/turnProjection.test.ts crates/deepx-tauri/src/components/conversation/TurnGroup.tsx crates/deepx-tauri/src/components/conversation/TurnGroup.test.tsx crates/deepx-tauri/src/components/process/ProcessDisclosure.tsx crates/deepx-tauri/src/components/process/ProcessDisclosure.test.tsx crates/deepx-tauri/src/components/MarkdownBody.tsx crates/deepx-tauri/src/components/MarkdownBody.test.tsx crates/deepx-tauri/src/components/conversation/ConversationTranscript.tsx crates/deepx-tauri/src/components/conversation/ConversationTranscript.test.tsx
git commit -m "fix(tauri): verify streaming transcript repair"
```

If no correction was required, do not create an empty commit.
