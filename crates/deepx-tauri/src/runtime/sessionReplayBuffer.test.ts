import { expect, it } from "vitest";
import { createSessionReplayBuffer } from "./sessionReplayBuffer";

it("applies replay before live events and removes exact overlap", () => {
  const buffer = createSessionReplayBuffer();
  const applied: string[] = [];
  const apply = (event: Record<string, unknown>) => applied.push(String(event.id));
  const overlap = { type: "round_complete", id: "complete" };

  buffer.begin("seed-a");
  buffer.handleLive("seed-a", overlap, apply);
  buffer.handleLive("seed-a", { type: "turn_end", id: "end" }, apply);
  expect(applied).toEqual([]);

  buffer.complete("seed-a", [
    { type: "turn_start", id: "start" },
    overlap,
  ], apply);

  expect(applied).toEqual(["start", "complete", "end"]);
});

it("drains buffered live events when replay is unavailable", () => {
  const buffer = createSessionReplayBuffer();
  const applied: string[] = [];
  buffer.begin("seed-a");
  buffer.handleLive("seed-a", { type: "turn_end", id: "end" }, event => {
    applied.push(String(event.id));
  });
  buffer.abort("seed-a", event => applied.push(String(event.id)));
  expect(applied).toEqual(["end"]);
});
