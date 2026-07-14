import { describe, expect, it } from "vitest";
import type { RawTurn } from "../store/rawSession";
import { projectTurn } from "./turnProjection";

function rawTurn(): RawTurn {
  return {
    turnId: "turn-1",
    userText: "fix it",
    status: "completed",
    startedAt: 100,
    endedAt: 900,
    interactions: [],
    rounds: [
      {
        roundNum: 0,
        isFinal: false,
        thinking: "inspect",
        answer: "intermediate",
        blocks: [],
        toolCalls: [{ id: "read-1", name: "read", args_display: "App.tsx", args_json: "{}" }],
        toolResults: { "read-1": { tool_call_id: "read-1", output: "source", success: true } },
        progress: {},
      },
      {
        roundNum: 1,
        isFinal: true,
        thinking: "verify",
        answer: "final answer",
        blocks: [],
        toolCalls: [],
        toolResults: {},
        progress: {},
      },
    ],
  };
}

describe("turn projection", () => {
  it("projects only the final round answer at top level", () => {
    const view = projectTurn(rawTurn());
    expect(view.process.items.some(item => item.kind === "assistant_progress")).toBe(true);
    expect(view.finalAnswer?.markdown).toBe("final answer");
    expect(view.process.elapsedMs).toBe(800);
  });

  it("keeps tool output and reasoning inside process items", () => {
    const view = projectTurn(rawTurn());
    expect(view.process.items.some(item => item.kind === "reasoning")).toBe(true);
    const tool = view.process.items.find(item => item.kind === "tool");
    expect(tool).toMatchObject({ kind: "tool", output: "source", success: true });
  });
});
