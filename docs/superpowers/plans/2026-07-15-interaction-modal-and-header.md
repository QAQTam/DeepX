# Interaction Modal and Header Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Center blocking interactions, clarify header controls, and stop completed process details from repeatedly collapsing.

**Architecture:** A reusable Portal-backed modal owns overlay semantics. `ChatView` separates blocking prompts from compact status, while `App` passes the existing workspace picker into the header. Process disclosure tracks status transitions rather than every projected object refresh.

**Tech Stack:** SolidJS, TypeScript, CSS, Vitest

## Global Constraints

- Permission and AskUser require explicit actions.
- High-risk approval remains red.
- No backend protocol changes.

---

### Task 1: Centered blocking interactions

**Files:** Create `InteractionModal.tsx`; modify `ChatView.tsx`, `interactions.css`; test interaction components.

**Interfaces:** `InteractionModal(props: { label: string; children: JSX.Element })` renders a Portal dialog.

- [x] Write failing tests proving Portal dialog semantics and ChatView modal placement.
- [x] Run targeted tests and confirm failure.
- [x] Implement the modal and keep compact status in `InteractionDock` only.
- [x] Run targeted tests and confirm success.

### Task 2: Header context controls

**Files:** Modify `ThreadHeader.tsx`, `ChatView.tsx`, `App.tsx`, `shell.css`; add `ThreadHeader.test.tsx`.

**Interfaces:** Header consumes `workspace`, `onChangeWorkspace`, `compacting`, and existing callbacks.

- [x] Write a failing test for visible workspace and compact labels.
- [x] Implement the header and pass the existing picker callback from App.
- [x] Run the targeted test.

### Task 3: Preserve manual process expansion

**Files:** Modify `ProcessDisclosure.tsx` and its test.

**Interfaces:** Existing public props remain unchanged.

- [x] Write a failing regression test: completed projection refresh must not close a manually reopened panel.
- [x] Limit auto-collapse to the transition into completed.
- [x] Run targeted and full frontend verification.
- [x] Commit and push the focused change.
