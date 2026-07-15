// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { expect, it, vi } from "vitest";
import type { TurnViewModel } from "../../presentation/turnProjection";
import TurnGroup from "./TurnGroup";

vi.mock("../MarkdownBody", () => ({
  default: (props: { content: string; final?: boolean }) => (
    <div data-markdown-final={props.final ? "true" : "false"}>{props.content}</div>
  ),
}));

it("renders prompt, per-round disclosures, and answer for a completed turn", () => {
  const host = document.createElement("div");
  const turn: TurnViewModel = {
    turnId: "turn-1",
    userPrompt: "请修复",
    status: "completed",
    elapsedMs: 4090,
    rounds: [
      { roundNum: 0, isFinal: true, processItems: [], answer: "已经完成" },
    ],
    interactions: [],
  };
  const dispose = render(() => <TurnGroup turn={turn} />, host);
  const group = host.querySelector("[data-turn]")!;
  const parts = Array.from(group.children).map(node => node.getAttribute("data-part")).filter(Boolean);
  expect(parts).toContain("user-prompt");
  expect(parts).toContain("assistant-answer");
  expect(host.textContent).toContain("已经完成");
  dispose();
});

it("renders stage conclusion outside its process disclosure", async () => {
  const host = document.createElement("div");
  const turn: TurnViewModel = {
    turnId: "turn-2",
    userPrompt: "继续",
    status: "running",
    rounds: [
      {
        roundNum: 0,
        isFinal: false,
        processItems: [{ kind: "reasoning", id: "r1", content: "思考中..." }],
        answer: "形成阶段结论",
      },
      {
        roundNum: 1,
        isFinal: true,
        processItems: [],
        answer: "最终结论输出中",
      },
    ],
    interactions: [],
  };
  const dispose = render(() => <TurnGroup turn={turn} />, host);

  await vi.waitFor(() =>
    expect(host.querySelector('[data-stage="true"]')?.textContent).toContain("形成阶段结论"),
  );
  // Stage answer is outside process disclosure
  expect(host.querySelector('[data-part="process"]')?.textContent).not.toContain("形成阶段结论");
  expect(host.querySelectorAll('[data-part="assistant-answer"]')).toHaveLength(2);
  dispose();
});

it("reacts when process items and the active answer arrive after mount", async () => {
  const host = document.createElement("div");
  const [turn, setTurn] = createSignal<TurnViewModel>({
    turnId: "turn-live",
    userPrompt: "开始",
    status: "running",
    rounds: [{ roundNum: 0, isFinal: false, processItems: [] }],
    interactions: [],
  });
  const dispose = render(() => <TurnGroup turn={turn()} />, host);

  expect(host.querySelector('[data-part="process"]')).toBeNull();

  setTurn(current => ({
    ...current,
    rounds: [{
      roundNum: 0,
      isFinal: false,
      processItems: [{ kind: "reasoning", id: "r-live", content: "正在分析" }],
      answer: "输出中",
    }],
  }));

  await vi.waitFor(() => {
    expect(host.querySelector('[data-part="process"]')?.textContent).toContain("分析了实现路径");
    expect(host.querySelector('[data-part="assistant-answer"]')?.textContent).toContain("输出中");
  });
  expect(host.querySelector('[data-stage="true"]')).toBeNull();
  expect(host.querySelector('[data-markdown-final="false"]')).not.toBeNull();

  setTurn(current => ({
    ...current,
    status: "completed",
    rounds: current.rounds.map(round => ({ ...round, isFinal: true })),
  }));

  await vi.waitFor(() =>
    expect(host.querySelector('[data-markdown-final="true"]')).not.toBeNull(),
  );
  dispose();
});
