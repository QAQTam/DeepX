// @vitest-environment jsdom

import { render } from "solid-js/web";
import { expect, it, vi } from "vitest";
import type { TurnViewModel } from "../../presentation/turnProjection";
import TurnGroup from "./TurnGroup";

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
