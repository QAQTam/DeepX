import { createSignal } from "solid-js";

export interface PermissionRequest {
  tool_call_id: string;
  tool_name: string;
  reason: string;
  paths: string[];
  category: string;
  level: number;
}

export interface QueuedPermission {
  seed: string;
  request: PermissionRequest;
}

export function createPermissionQueue() {
  const [items, setItems] = createSignal<QueuedPermission[]>([]);
  const active = () => items()[0] ?? null;

  function enqueue(seed: string, request: PermissionRequest) {
    if (!seed || !request.tool_call_id) return;
    setItems((current) => {
      const duplicate = current.some(
        (item) => item.seed === seed && item.request.tool_call_id === request.tool_call_id,
      );
      return duplicate ? current : [...current, { seed, request }];
    });
  }

  function resolve(seed: string, toolCallId: string): boolean {
    const current = items();
    const first = current[0];
    if (!first || first.seed !== seed || first.request.tool_call_id !== toolCallId) return false;
    setItems(current.slice(1));
    return true;
  }

  function clearSeed(seed: string) {
    setItems((current) => current.filter((item) => item.seed !== seed));
  }

  function clear() {
    setItems([]);
  }

  return { active, enqueue, resolve, clearSeed, clear };
}
