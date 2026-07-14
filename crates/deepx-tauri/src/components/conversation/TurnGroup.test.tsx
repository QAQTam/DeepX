// @vitest-environment jsdom

import { render } from "solid-js/web";
import { expect, it } from "vitest";
import type { TurnViewModel } from "../../presentation/turnProjection";
import TurnGroup from "./TurnGroup";

it("renders only prompt, process disclosure, and final answer for a completed turn", () => {
  const host = document.createElement("div");
  const turn: TurnViewModel = {
    turnId: "turn-1",
    userPrompt: "请修复",
    process: { status: "completed", elapsedMs: 4090, items: [] },
    finalAnswer: { markdown: "已经完成" },
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
