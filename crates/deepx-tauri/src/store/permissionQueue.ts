import { createSignal } from "solid-js";
import type { PermissionRisk } from "../lib/types";

export interface PermissionRequest {
  tool_call_id: string;
  tool_name: string;
  reason: string;
  paths: string[];
  category: string;
  level: number;
  risk: PermissionRisk;
  consequence: string;
}

export interface QueuedPermission {
  seed: string;
  request: PermissionRequest;
}

export interface PermissionQueueProgress {
  current: number;
  total: number;
}

interface BatchProgress {
  resolved: number;
  total: number;
}

export function createPermissionQueue() {
  const [items, setItems] = createSignal<QueuedPermission[]>([]);
  const [batches, setBatches] = createSignal<Record<string, BatchProgress>>({});
  const active = () => items()[0] ?? null;

  function progress(seed: string): PermissionQueueProgress | null {
    const batch = batches()[seed];
    if (!batch || !items().some((item) => item.seed === seed)) return null;
    return {
      current: Math.min(batch.resolved + 1, batch.total),
      total: batch.total,
    };
  }

  function enqueue(seed: string, request: PermissionRequest) {
    if (!seed || !request.tool_call_id) return;
    setItems((current) => {
      const duplicate = current.some(
        (item) => item.seed === seed && item.request.tool_call_id === request.tool_call_id,
      );
      if (duplicate) return current;
      const continuingBatch = current.some((item) => item.seed === seed);
      setBatches((batches) => {
        const previous = continuingBatch ? batches[seed] : undefined;
        return {
          ...batches,
          [seed]: {
            resolved: previous?.resolved ?? 0,
            total: (previous?.total ?? 0) + 1,
          },
        };
      });
      return [...current, { seed, request }];
    });
  }

  function resolve(seed: string, toolCallId: string): boolean {
    const current = items();
    const first = current[0];
    if (!first || first.seed !== seed || first.request.tool_call_id !== toolCallId) return false;
    setItems(current.slice(1));
    setBatches((batches) => ({
      ...batches,
      [seed]: {
        resolved: (batches[seed]?.resolved ?? 0) + 1,
        total: batches[seed]?.total ?? 1,
      },
    }));
    return true;
  }

  function clearSeed(seed: string) {
    setItems((current) => current.filter((item) => item.seed !== seed));
    setBatches((batches) => {
      const next = { ...batches };
      delete next[seed];
      return next;
    });
  }

  function clear() {
    setItems([]);
    setBatches({});
  }

  return { active, progress, enqueue, resolve, clearSeed, clear };
}
