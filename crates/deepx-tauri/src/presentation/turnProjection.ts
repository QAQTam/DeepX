import type { RawRound, RawTurn } from "../store/rawSession";
import type { ToolCallDef } from "../lib/types";
import { aggregateProcessItems, type ProcessItem } from "./processAggregation";

export type RoundRenderEntry =
  | { kind: "assistant"; id: string; markdown: string; streaming: boolean }
  | { kind: "process"; id: string; items: ProcessItem[]; hasTools: boolean };

export type RoundViewModel = {
  roundNum: number;
  isFinal: boolean;
  entries: RoundRenderEntry[];
};

export type TurnViewModel = {
  turnId: string;
  userPrompt: string;
  status: string;
  /** Total elapsed time across all rounds, or undefined until complete. */
  elapsedMs?: number;
  rounds: RoundViewModel[];
  /** Resolved interactions (ask/plan), shown at turn level. */
  interactions: ProcessItem[];
  /** Total tokens used in this turn, from API usage info. */
  totalTokens?: number;
  /** Approximate tokens per second (total tokens / total elapsed). */
  tokensPerSec?: number;
};

export function toolFamily(name: string): string {
  if (["read", "list", "search", "diff", "explore_scan"].includes(name)) return "read";
  if (["write", "edit", "edit_block", "delete"].includes(name)) return "write";
  if (["web", "web_search", "web_fetch"].includes(name)) return "web";
  if (["exec", "exec_run", "spawn_subagent"].includes(name)) return "exec";
  return "tool";
}

function reasoningItem(turnId: string, roundNum: number, ordinal: number, content: string): ProcessItem {
  return {
    kind: "reasoning",
    id: `${turnId}-reasoning-${roundNum}-${ordinal}`,
    content,
  };
}

function toolItem(round: RawRound, call: ToolCallDef): Extract<ProcessItem, { kind: "tool" }> {
  const result = round.toolResults[call.id];
  return {
    kind: "tool",
    id: call.id,
    family: toolFamily(call.name),
    toolName: call.name,
    summary: call.args_display || call.name,
    argsJson: call.args_json,
    output: result?.output,
    progress: round.progress[call.id]?.chunks,
    success: result?.success,
  };
}

function projectRoundEntries(
  turn: RawTurn,
  round: RawRound,
  streaming: boolean,
): RoundRenderEntry[] {
  const entries: RoundRenderEntry[] = [];
  let processItems: ProcessItem[] = [];
  let ordinal = 0;

  const flushProcess = () => {
    if (processItems.length === 0) return;
    const items = aggregateProcessItems(processItems);
    entries.push({
      kind: "process",
      id: `${turn.turnId}-round-${round.roundNum}-process-${ordinal++}`,
      items,
      hasTools: processItems.some(item => item.kind === "tool"),
    });
    processItems = [];
  };

  if (round.blocks.length > 0) {
    for (const block of round.blocks) {
      switch (block.type) {
        case "reasoning":
          if (block.content.trim()) {
            processItems.push(reasoningItem(turn.turnId, round.roundNum, processItems.length, block.content));
          }
          break;
        case "tool":
          processItems.push(toolItem(round, block.card));
          break;
        case "text":
          if (!block.content.trim()) break;
          flushProcess();
          entries.push({
            kind: "assistant",
            id: `${turn.turnId}-round-${round.roundNum}-assistant-${ordinal++}`,
            markdown: block.content,
            streaming: false,
          });
          break;
      }
    }
    flushProcess();
    return entries;
  }

  // Streaming previews are intentionally phase-exclusive. A provider may emit
  // reasoning deltas before its tool-call preview; retaining both makes the UI
  // look as though it is thinking and executing at once.
  if (round.phase !== "tool_calling" && round.thinking.trim()) {
    processItems.push(reasoningItem(turn.turnId, round.roundNum, 0, round.thinking));
  }
  if (round.phase !== "thinking") {
    for (const call of round.toolCalls) {
      processItems.push(toolItem(round, call));
    }
  }
  flushProcess();

  if (round.answer.trim()) {
    entries.push({
      kind: "assistant",
      id: `${turn.turnId}-round-${round.roundNum}-assistant-${ordinal}`,
      markdown: round.answer,
      streaming,
    });
  }

  return entries;
}

export function projectTurn(rawTurn: RawTurn): TurnViewModel {
  const rounds = rawTurn.rounds.map((round, index) => ({
    roundNum: round.roundNum,
    isFinal: round.isFinal,
    entries: projectRoundEntries(
      rawTurn,
      round,
      rawTurn.status === "running" && index === rawTurn.rounds.length - 1,
    ),
  }));

  // Resolved interactions (ask/plan) belong to the turn, not a specific round.
  const interactions: ProcessItem[] = [];
  for (const interaction of rawTurn.interactions) {
    if (interaction.kind === "permission") continue;
    interactions.push({
      kind: "interaction",
      id: interaction.id,
      label: interaction.kind,
      resolution: interaction.resolution,
    });
  }

  const elapsedMs = rawTurn.startedAt !== undefined && rawTurn.endedAt !== undefined
    ? Math.max(0, rawTurn.endedAt - rawTurn.startedAt)
    : undefined;

  const totalTokens = rawTurn.usage?.total_tokens;
  const tokensPerSec = totalTokens !== undefined && elapsedMs !== undefined && elapsedMs > 0
    ? Math.round(totalTokens / (elapsedMs / 1000))
    : undefined;

  return {
    turnId: rawTurn.turnId,
    userPrompt: rawTurn.userText,
    status: rawTurn.status,
    elapsedMs,
    rounds,
    interactions,
    totalTokens,
    tokensPerSec,
  };
}
