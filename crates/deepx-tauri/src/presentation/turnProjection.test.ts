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
  it("projects assistant stage text directly and keeps the last answer as final", () => {
    const view = projectTurn(rawTurn());
    expect(view.process.items.some(item => item.kind === "assistant_progress")).toBe(false);
    expect(view.stageAnswers).toEqual([{ roundNum: 0, markdown: "intermediate" }]);
    expect(view.finalAnswer?.markdown).toBe("final answer");
    expect(view.finalAnswer?.streaming).toBe(false);
    expect(view.process.elapsedMs).toBe(800);
  });

  it("projects the latest streaming answer directly as the final answer candidate", () => {
    const turn = rawTurn();
    turn.status = "running";
    turn.rounds[1].isFinal = false;
    turn.rounds[1].answer = "forming conclusion";

    const view = projectTurn(turn);

    expect(view.finalAnswer).toEqual({ markdown: "forming conclusion", streaming: true });
    expect(view.process.items.some(item => item.kind === "assistant_progress")).toBe(false);
  });

  it("keeps tool output and reasoning inside process items", () => {
    const view = projectTurn(rawTurn());
    expect(view.process.items.some(item => item.kind === "reasoning")).toBe(true);
    const tool = view.process.items.find(item => item.kind === "tool");
    expect(tool).toMatchObject({ kind: "tool", output: "source", success: true });
  });

  it("does not expose permission audit resolutions as chat process items", () => {
    const turn = rawTurn();
    turn.interactions = [
      { id: "exec-1", kind: "permission", resolution: "approved", at: 500 },
    ];

    const view = projectTurn(turn);

    expect(view.process.items).not.toContainEqual(
      expect.objectContaining({ kind: "interaction", label: "permission" }),
    );
  });
});
