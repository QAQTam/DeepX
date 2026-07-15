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
  it("separates stage answer and final answer across rounds", () => {
    const view = projectTurn(rawTurn());
    expect(view.rounds).toHaveLength(2);

    // Round 0: stage answer
    expect(view.rounds[0].answer).toBe("intermediate");
    expect(view.rounds[0].isFinal).toBe(false);

    // Round 1: final answer
    expect(view.rounds[1].answer).toBe("final answer");
    expect(view.rounds[1].isFinal).toBe(true);

    expect(view.elapsedMs).toBe(800);
    expect(view.status).toBe("completed");
  });

  it("rounds with only a streaming answer are still projected", () => {
    const turn = rawTurn();
    turn.status = "running";
    turn.rounds[1].isFinal = false;
    turn.rounds[1].answer = "forming conclusion";

    const view = projectTurn(turn);

    expect(view.rounds[1].answer).toBe("forming conclusion");
    expect(view.rounds[1].isFinal).toBe(false);
    expect(view.status).toBe("running");
  });

  it("keeps tool output and reasoning in per-round process items", () => {
    const view = projectTurn(rawTurn());

    const r0 = view.rounds[0];
    expect(r0.processItems.some(item => item.kind === "reasoning")).toBe(true);
    const tool = r0.processItems.find(item => item.kind === "tool");
    expect(tool).toMatchObject({ kind: "tool", output: "source", success: true });
  });

  it("does not expose permission audit resolutions as process items", () => {
    const turn = rawTurn();
    turn.interactions = [
      { id: "exec-1", kind: "permission", resolution: "approved", at: 500 },
    ];

    const view = projectTurn(turn);

    // Permission interactions should not appear in any round's items
    for (const round of view.rounds) {
      expect(round.processItems).not.toContainEqual(
        expect.objectContaining({ kind: "interaction", label: "permission" }),
      );
    }
  });
});
