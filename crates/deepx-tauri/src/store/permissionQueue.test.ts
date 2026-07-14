import { createRoot } from "solid-js";
import { describe, expect, it } from "vitest";

import { createPermissionQueue } from "./permissionQueue";

function request(id: string) {
  return {
    tool_call_id: id,
    tool_name: "shell_command",
    reason: "test",
    paths: [],
    category: "exec",
    level: 2,
    risk: "high" as const,
    consequence: "May execute arbitrary actions.",
  };
}

describe("permission request queue", () => {
  it("keeps requests ordered, deduplicated, and bound to their listener seed", () => {
    createRoot((dispose) => {
      const queue = createPermissionQueue();
      queue.enqueue("seed-a", request("call-1"));
      queue.enqueue("seed-b", request("call-2"));
      queue.enqueue("seed-a", request("call-1"));

      expect(queue.active()).toMatchObject({ seed: "seed-a", request: { tool_call_id: "call-1" } });
      expect(queue.resolve("seed-b", "call-2")).toBe(false);
      expect(queue.active()?.request.tool_call_id).toBe("call-1");
      expect(queue.resolve("seed-a", "call-1")).toBe(true);
      expect(queue.active()).toMatchObject({ seed: "seed-b", request: { tool_call_id: "call-2" } });
      expect(queue.resolve("seed-b", "call-2")).toBe(true);
      expect(queue.active()).toBeNull();
      dispose();
    });
  });

  it("clears only the invalidated session while preserving other sessions", () => {
    createRoot((dispose) => {
      const queue = createPermissionQueue();
      queue.enqueue("seed-a", request("call-1"));
      queue.enqueue("seed-b", request("call-2"));

      queue.clearSeed("seed-a");

      expect(queue.active()).toMatchObject({ seed: "seed-b", request: { tool_call_id: "call-2" } });
      dispose();
    });
  });

  it("reports stable progress across a four-request permission batch", () => {
    createRoot((dispose) => {
      const queue = createPermissionQueue();
      for (let index = 1; index <= 4; index += 1) {
        queue.enqueue("seed-a", request(`call-${index}`));
      }

      expect(queue.progress("seed-a")).toEqual({ current: 1, total: 4 });
      expect(queue.resolve("seed-a", "call-1")).toBe(true);
      expect(queue.progress("seed-a")).toEqual({ current: 2, total: 4 });
      dispose();
    });
  });
});
