// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "@solidjs/web";
import { expect, it, vi } from "vitest";
import type { TurnViewModel } from "../../presentation/turnProjection";
import TurnGroup from "./TurnGroup";

vi.mock("../../i18n", () => ({
  useI18n: () => ({
    t: () => ({ review: { changedFiles: "Changed {n} files", reviewChanges: "Review changes" } }),
  }),
}));

vi.mock("../MarkdownBody", () => ({
  default: (props: { content: string; final?: boolean }) => (
    <div data-markdown-final={props.final ? "true" : "false"}>{props.content}</div>
  ),
}));

function processEntry() {
  return {
    kind: "process" as const,
    id: "process-0",
    hasTools: true,
    items: [{
      kind: "tool" as const,
      id: "read-1",
      family: "read",
      toolName: "read",
      summary: "App.tsx",
      success: true,
    }],
  };
}

it("renders protocol-ordered assistant chats outside a tool process", () => {
  const host = document.createElement("div");
  const turn: TurnViewModel = {
    turnId: "turn-1",
    userPrompt: "请修复",
    status: "completed",
    elapsedMs: 4090,
    rounds: [{
      roundNum: 0,
      isFinal: false,
      entries: [
        { kind: "assistant", id: "assistant-1", markdown: "调用前说明", streaming: false },
        processEntry(),
        { kind: "assistant", id: "assistant-2", markdown: "调用后说明", streaming: false },
      ],
    }],
    interactions: [],
  };
  const dispose = render(() => <TurnGroup turn={turn} />, host);

  const group = host.querySelector("[data-turn]")!;
  expect(group.querySelectorAll('[data-part="assistant-answer"]')).toHaveLength(2);
  const process = group.querySelector('[data-part="process"]')!;
  expect(process.textContent).not.toContain("调用前说明");
  expect(process.textContent).not.toContain("调用后说明");
  expect(group.textContent).toContain("调用前说明");
  expect(group.textContent).toContain("调用后说明");
  dispose();
});

it("combines all process entries in a turn into one collapsed activity summary", () => {
  const host = document.createElement("div");
  const turn: TurnViewModel = {
    turnId: "turn-summary",
    userPrompt: "完成任务",
    status: "completed",
    rounds: [{
      roundNum: 0,
      isFinal: false,
      entries: [processEntry(), { ...processEntry(), id: "process-1" }],
    }],
    interactions: [],
  };
  const dispose = render(() => <TurnGroup turn={turn} />, host);

  expect(host.querySelectorAll('[data-part="process"]')).toHaveLength(1);
  expect(host.textContent).toContain("完成 2 项操作");
  dispose();
});

it("renders a live answer-only entry as streaming Markdown", async () => {
  const host = document.createElement("div");
  const [turn, setTurn] = createSignal<TurnViewModel>({
    turnId: "turn-live",
    userPrompt: "开始",
    status: "running",
    rounds: [{
      roundNum: 0,
      isFinal: false,
      entries: [{ kind: "assistant", id: "live-answer", markdown: "输出中", streaming: true }],
    }],
    interactions: [],
  });
  const dispose = render(() => <TurnGroup turn={turn()} />, host);

  expect(host.querySelector('[data-markdown-final="false"]')?.textContent).toContain("输出中");

  setTurn(current => ({
    ...current,
    rounds: [{
      ...current.rounds[0]!,
      entries: [
        processEntry(),
        { kind: "assistant", id: "live-answer", markdown: "输出已更新", streaming: true },
      ],
    }],
  }));

  await vi.waitFor(() => {
    expect(host.querySelector('[data-part="process"]')).not.toBeNull();
    expect(host.querySelector('[data-markdown-final="false"]')?.textContent).toContain("输出已更新");
  });
  dispose();
});

it("keeps assistant chats visible when the turn completes", async () => {
  const host = document.createElement("div");
  const [turn, setTurn] = createSignal<TurnViewModel>({
    turnId: "turn-done",
    userPrompt: "开始",
    status: "running",
    rounds: [{
      roundNum: 0,
      isFinal: false,
      entries: [processEntry(), { kind: "assistant", id: "answer", markdown: "保留回答", streaming: true }],
    }],
    interactions: [],
  });
  const dispose = render(() => <TurnGroup turn={turn()} />, host);

  setTurn(current => ({
    ...current,
    status: "completed",
    rounds: [{
      ...current.rounds[0]!,
      entries: [processEntry(), { kind: "assistant", id: "answer", markdown: "保留回答", streaming: false }],
    }],
  }));

  await vi.waitFor(() => expect(host.querySelector('[data-part="assistant-answer"]')?.textContent).toContain("保留回答"));
  expect(host.querySelector('[data-part="process"]')).not.toBeNull();
  dispose();
});
