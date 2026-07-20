import { createRoot } from "solid-js";
import { expect, it, vi } from "vitest";
import { createRawSessionState } from "./sessionEventReducer";
import { createSessionRegistry } from "./sessionRegistry";

function memoryStorage() {
  const values = new Map<string, string>();
  return {
    values,
    storage: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
      removeItem: (key: string) => { values.delete(key); },
    },
  };
}

it("hydrates once, remaps without replacing the entry, and removes frontend resources", () => {
  // @ts-expect-error SolidJS 2.x: ownedWrite option for test roots
  createRoot(dispose => {
    const { values, storage } = memoryStorage();
    const restored = createRawSessionState("old");
    restored.turns.push({
      turnId: "t1", userText: "restored", status: "completed", rounds: [], interactions: [],
    });
    values.set("deepx:reload:v3:old", JSON.stringify({ version: 3, state: restored }));

    const registry = createSessionRegistry({ storage });
    const entry = registry.ensure("old");
    const unlisten = vi.fn();
    entry.attachListener(unlisten);

    expect(registry.ensure("old")).toBe(entry);
    expect(entry.state().turns[0].turnId).toBe("t1");
    expect(registry.remap("old", "new")).toBe(entry);
    expect(entry.state().seed).toBe("new");
    expect(entry.state().turns[0].turnId).toBe("t1");

    registry.remove("new");
    expect(unlisten).toHaveBeenCalledOnce();
    expect(registry.get("new")).toBeUndefined();
    expect(values.has("deepx:reload:v3:old")).toBe(false);
    expect(values.has("deepx:reload:v3:new")).toBe(false);
    dispose();
  }, { ownedWrite: true });
});

it("disposes only frontend-owned runtimes and listeners", () => {
  const { storage } = memoryStorage();
  const registry = createSessionRegistry({ storage });
  const entry = registry.ensure("seed-a");
  const unlisten = vi.fn();
  entry.attachListener(unlisten);
  registry.disposeView();
  expect(unlisten).toHaveBeenCalledOnce();
  expect(registry.entries()).toEqual([]);
});

it("keeps the new-seed snapshot when remap follows session_created reduction", () => {
  const { values, storage } = memoryStorage();
  const registry = createSessionRegistry({ storage });
  const entry = registry.ensure("old");
  entry.runtime.push({ type: "session_created", seed: "new" });

  registry.remap("old", "new");

  expect(values.has("deepx:reload:v3:old")).toBe(false);
  expect(values.has("deepx:reload:v3:new")).toBe(true);
  expect(registry.get("new")).toBe(entry);
});
