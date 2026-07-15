import { describe, expect, it } from "vitest";
import { createRawSessionState } from "./sessionEventReducer";
import {
  createSessionEventRuntime,
  loadReloadSnapshot,
  type ReloadStorage,
} from "./sessionEventRuntime";

class MemoryStorage implements ReloadStorage {
  private values = new Map<string, string>();
  getItem(key: string) { return this.values.get(key) ?? null; }
  setItem(key: string, value: string) { this.values.set(key, value); }
  removeItem(key: string) { this.values.delete(key); }
}

describe("sessionEventRuntime", () => {
  it("commits streaming deltas once per frame and terminal events immediately", () => {
    const storage = new MemoryStorage();
    const commits: string[] = [];
    const scheduled: Array<() => void> = [];
    const runtime = createSessionEventRuntime({
      initialState: createRawSessionState("seed-a"),
      commit: state => commits.push(state.turns[0]?.rounds[0]?.answer ?? ""),
      storage,
      schedule: flush => scheduled.push(flush),
      now: () => 100,
    });

    runtime.push({ type: "turn_start", turn_id: "t1", user_text: "go" });
    expect(commits).toHaveLength(1);

    runtime.push({ type: "round_delta", turn_id: "t1", round_num: 0, kind: "answering", delta: "A" });
    runtime.push({ type: "round_delta", turn_id: "t1", round_num: 0, kind: "answering", delta: "B" });
    expect(commits).toHaveLength(1);
    expect(scheduled).toHaveLength(1);

    scheduled[0]!();
    expect(commits[commits.length - 1]).toBe("AB");

    runtime.push({ type: "turn_end", turn_id: "t1" });
    expect(runtime.current().turns[0].status).toBe("completed");
    expect(commits).toHaveLength(3);
  });

  it("flushes on dispose and restores the last twenty turns", () => {
    const storage = new MemoryStorage();
    const state = createRawSessionState("seed-a");
    state.turns = Array.from({ length: 25 }, (_, index) => ({
      turnId: `t${index}`,
      userText: `${index}`,
      status: "completed" as const,
      rounds: [],
      interactions: [],
    }));
    const runtime = createSessionEventRuntime({
      initialState: state,
      commit: () => {},
      storage,
      schedule: () => {},
    });

    runtime.dispose();
    const restored = loadReloadSnapshot(storage, "seed-a");
    expect(restored?.turns).toHaveLength(20);
    expect(restored?.turns[0].turnId).toBe("t5");
  });

  it("rejects corrupt or wrong-seed snapshots", () => {
    const storage = new MemoryStorage();
    storage.setItem("deepx:reload:v1:seed-a", "not-json");
    expect(loadReloadSnapshot(storage, "seed-a")).toBeUndefined();

    storage.setItem("deepx:reload:v1:seed-a", JSON.stringify({
      version: 1,
      state: { ...createRawSessionState("seed-b"), seed: "seed-b" },
    }));
    expect(loadReloadSnapshot(storage, "seed-a")).toBeUndefined();
  });
});
