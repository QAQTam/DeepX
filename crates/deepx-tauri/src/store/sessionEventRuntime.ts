import type { Agent2Ui } from "../lib/types";
import type { RawSessionState } from "./rawSession";
import { reduceAgentEvent } from "./sessionEventReducer";

export type ReloadStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;
export type ScheduleFlush = (flush: () => void) => void;

const SNAPSHOT_VERSION = 4;
const SNAPSHOT_PREFIX = "deepx:reload:v4:";
const LEGACY_SNAPSHOT_PREFIXES = ["deepx:reload:v1:", "deepx:reload:v2:", "deepx:reload:v3:"];
const MAX_RELOAD_TURNS = 20;
const MAX_PROGRESS_CHUNKS = 200;

const IMMEDIATE_EVENT_TYPES = new Set<Agent2Ui["type"]>([
  "turn_start",
  "turn_end",
  "round_complete",
  "tool_results",
  "session_restored",
  "more_turns",
  "session_created",
  "error",
  "permission_request",
  "ask_user",
  "ask_resolved",
  "ask_rejected",
  "plan_submitted",
  "plan_resolved",
  "compact_start",
  "compact_end",
  "cancelled",
  "done",
  "ready",
]);

function reloadKey(seed: string): string {
  return `${SNAPSHOT_PREFIX}${seed}`;
}

function compactReloadState(state: RawSessionState): RawSessionState {
  return {
    ...state,
    turns: state.turns.slice(-MAX_RELOAD_TURNS).map(turn => ({
      ...turn,
      rounds: turn.rounds.map(round => ({
        ...round,
        progress: Object.fromEntries(
          Object.entries(round.progress).map(([id, progress]) => [
            id,
            { chunks: progress.chunks.slice(-MAX_PROGRESS_CHUNKS) },
          ]),
        ),
      })),
    })),
  };
}

function saveReloadSnapshot(storage: ReloadStorage, state: RawSessionState): void {
  try {
    storage.setItem(reloadKey(state.seed), JSON.stringify({
      version: SNAPSHOT_VERSION,
      state: compactReloadState(state),
    }));
  } catch (error) {
    console.warn("[reload-snapshot] save failed", error);
  }
}

export function loadReloadSnapshot(
  storage: ReloadStorage,
  seed: string,
): RawSessionState | undefined {
  try {
    for (const prefix of LEGACY_SNAPSHOT_PREFIXES) storage.removeItem(`${prefix}${seed}`);
    const raw = storage.getItem(reloadKey(seed));
    if (!raw) return undefined;
    const parsed = JSON.parse(raw) as { version?: number; state?: RawSessionState };
    if (
      parsed.version !== SNAPSHOT_VERSION ||
      parsed.state?.seed !== seed ||
      !Array.isArray(parsed.state.turns)
    ) {
      storage.removeItem(reloadKey(seed));
      return undefined;
    }
    return parsed.state;
  } catch {
    storage.removeItem(reloadKey(seed));
    return undefined;
  }
}

export function removeReloadSnapshot(storage: ReloadStorage, seed: string): void {
  storage.removeItem(reloadKey(seed));
}

export interface SessionEventRuntime {
  push(event: Agent2Ui): void;
  update(update: (state: RawSessionState) => RawSessionState): void;
  flush(): void;
  dispose(): void;
  current(): RawSessionState;
}

export function createSessionEventRuntime(options: {
  initialState: RawSessionState;
  commit: (state: RawSessionState) => void;
  storage: ReloadStorage;
  schedule?: ScheduleFlush;
  now?: () => number;
}): SessionEventRuntime {
  let state = options.initialState;
  let scheduled = false;
  let disposed = false;
  const now = options.now ?? Date.now;
  const schedule = options.schedule ?? ((flush: () => void) => {
    requestAnimationFrame(flush);
  });

  const commitAndPersist = () => {
    options.commit(state);
    saveReloadSnapshot(options.storage, state);
  };

  const flush = () => {
    if (disposed) return;
    scheduled = false;
    commitAndPersist();
  };

  const scheduleCommit = () => {
    if (scheduled || disposed) return;
    scheduled = true;
    schedule(() => {
      if (disposed || !scheduled) return;
      flush();
    });
  };

  return {
    push(event) {
      if (disposed) return;
      state = reduceAgentEvent(state, event, now());
      if (IMMEDIATE_EVENT_TYPES.has(event.type)) flush();
      else scheduleCommit();
    },
    update(update) {
      if (disposed) return;
      state = update(state);
      flush();
    },
    flush,
    dispose() {
      if (disposed) return;
      flush();
      disposed = true;
    },
    current() {
      return state;
    },
  };
}
