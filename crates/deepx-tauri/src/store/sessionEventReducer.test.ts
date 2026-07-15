import { describe, expect, it } from "vitest";
import type { Agent2Ui } from "../lib/types";
import { createRawSessionState, reduceAgentEvent } from "./sessionEventReducer";

describe("sessionEventReducer", () => {
  it("retains final-round, code-delta, and ordered exec facts", () => {
    let state = createRawSessionState("seed-a");
    state = reduceAgentEvent(state, {
      type: "turn_start", turn_id: "t1", user_text: "go",
    }, 100);
    state = reduceAgentEvent(state, {
      type: "round_complete",
      turn_id: "t1",
      round_num: 1,
      answer: "done",
      tool_calls: [{ id: "exec-1", name: "exec_run", args_display: "pnpm test", args_json: "{}" }],
      blocks: [{ type: "text", content: "done" }],
      is_final: true,
    }, 200);
    state = reduceAgentEvent(state, {
      type: "exec_progress", tool_call_id: "exec-1", stream: "stdout", seq: 2n, chunk: "B",
    }, 205);
    state = reduceAgentEvent(state, {
      type: "exec_progress", tool_call_id: "exec-1", stream: "stderr", seq: 1n, chunk: "E",
    }, 206);
    state = reduceAgentEvent(state, {
      type: "code_delta", lines_added: 7, lines_removed: 2,
      files_created: 0, files_deleted: 0, file: "src/App.tsx",
    }, 210);

    expect(state.turns[0].rounds[0].isFinal).toBe(true);
    expect(state.turns[0].rounds[0].progress["exec-1"].chunks.map(item => item.seq)).toEqual([1, 2]);
    expect(state.environment.linesAdded).toBe(7);
  });

  it("restores final round facts and pending permission risk", () => {
    let state = createRawSessionState("seed-a");
    state = reduceAgentEvent(state, {
      type: "session_restored",
      seed: "seed-a",
      turns: [{
        turn_id: "t1",
        user_text: "restore",
        rounds: [{ round_num: 0, is_final: true, thinking: null, answer: "restored", tool_calls: [], tool_results: [] }],
      }],
      tokens_used: 10,
      cache_hit_pct: 0,
      total_turns: 1,
      has_more: false,
    }, 100);
    state = reduceAgentEvent(state, {
      type: "permission_request",
      tool_call_id: "danger-1",
      tool_name: "exec_run",
      reason: "Run command",
      paths: [],
      category: "exec",
      level: 1,
      risk: "high",
      consequence: "May execute arbitrary actions.",
    }, 110);

    expect(state.turns[0].rounds[0].isFinal).toBe(true);
    expect(state.pendingInteraction?.kind).toBe("permission");
    expect(state.pendingInteraction?.kind).toBe("permission");
    if (state.pendingInteraction?.kind === "permission") {
      expect(state.pendingInteraction.risk).toBe("high");
    }
  });

  it("accepts every generated event variant without dropping session identity", () => {
    const lifecycleEvents: Agent2Ui[] = [
      { type: "ready" },
      { type: "pong" },
      { type: "done" },
      { type: "shutdown_ack" },
    ];
    const final = lifecycleEvents.reduce(
      (state, event) => reduceAgentEvent(state, event, 1),
      createRawSessionState("seed-a"),
    );
    expect(final.seed).toBe("seed-a");
  });

  it("tracks plan review as a waiting interaction and resolves it", () => {
    let state = createRawSessionState("seed-a");
    state = reduceAgentEvent(state, {
      type: "turn_start", turn_id: "t-plan", user_text: "plan",
    }, 100);
    state = reduceAgentEvent(state, {
      type: "plan_submitted", call_id: "plan-1", plan_content: "# Plan",
    }, 110);

    expect(state.turns[0].status).toBe("waiting");
    expect(state.pendingInteraction).toEqual({ kind: "plan", id: "plan-1" });

    state = reduceAgentEvent(state, {
      type: "plan_resolved", call_id: "plan-1", approved: true,
    }, 120);

    expect(state.turns[0].status).toBe("running");
    expect(state.pendingInteraction).toBeNull();
    expect(state.turns[0].interactions[state.turns[0].interactions.length - 1]).toMatchObject({
      id: "plan-1", kind: "plan", resolution: "approved",
    });
  });

  it("does not duplicate consecutive notices when lifecycle events are replayed", () => {
    let state = createRawSessionState("seed-a");
    const event = { type: "error" as const, message: "agent exited" };
    state = reduceAgentEvent(state, event, 100);
    state = reduceAgentEvent(state, event, 110);
    expect(state.notices).toHaveLength(1);
  });
});
