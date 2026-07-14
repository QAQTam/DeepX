// @vitest-environment jsdom

import { render } from "solid-js/web";
import { expect, it, vi } from "vitest";
import type { TurnViewModel } from "../../presentation/turnProjection";
import TurnGroup from "./TurnGroup";

it("renders only prompt, process disclosure, and final answer for a completed turn", () => {
  const host = document.createElement("div");
  const turn: TurnViewModel = {
    turnId: "turn-1",
    userPrompt: "请修复",
    process: { status: "completed", elapsedMs: 4090, items: [] },
    stageAnswers: [],
    finalAnswer: { markdown: "已经完成", streaming: false },
  };
  const dispose = render(() => <TurnGroup turn={turn} />, host);
  const group = host.querySelector("[data-turn]")!;
  expect(Array.from(group.children).map(node => node.getAttribute("data-part"))).toEqual([
    "user-prompt", "process", "assistant-answer",
  ]);
  expect(host.querySelector(".tool-card")).toBeNull();
  expect(host.querySelector(".msg-avatar")).toBeNull();
  expect(host.textContent).toContain("已处理 4.1s");
  dispose();
});

it("renders stage conclusions as assistant chat outside the process disclosure", async () => {
  const host = document.createElement("div");
  const turn: TurnViewModel = {
    turnId: "turn-2",
    userPrompt: "继续",
    process: { status: "running", items: [] },
    stageAnswers: [{ roundNum: 0, markdown: "形成阶段结论" }],
    finalAnswer: { markdown: "最终结论输出中", streaming: true },
  };
  const dispose = render(() => <TurnGroup turn={turn} />, host);

  await vi.waitFor(() => expect(host.querySelector('[data-stage="true"]')?.textContent).toContain("形成阶段结论"));
  expect(host.querySelector('[data-part="process"]')?.textContent).not.toContain("形成阶段结论");
  expect(host.querySelectorAll('[data-part="assistant-answer"]')).toHaveLength(2);
  dispose();
});
