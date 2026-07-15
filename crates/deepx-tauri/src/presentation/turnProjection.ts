import type { RawTurn } from "../store/rawSession";
import { aggregateProcessItems, type ProcessItem } from "./processAggregation";

export type RoundViewModel = {
  roundNum: number;
  isFinal: boolean;
  processItems: ProcessItem[];
  answer?: string;
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

export function projectTurn(rawTurn: RawTurn): TurnViewModel {
  const rounds: RoundViewModel[] = [];

  for (const round of rawTurn.rounds) {
    const items: ProcessItem[] = [];

    if (round.thinking.trim()) {
      items.push({
        kind: "reasoning",
        id: `${rawTurn.turnId}-reasoning-${round.roundNum}`,
        content: round.thinking,
      });
    }
    for (const call of round.toolCalls) {
      const result = round.toolResults[call.id];
      items.push({
        kind: "tool",
        id: call.id,
        family: toolFamily(call.name),
        toolName: call.name,
        summary: call.args_display || call.name,
        argsJson: call.args_json,
        output: result?.output,
        progress: round.progress[call.id]?.chunks,
        success: result?.success,
      });
    }

    rounds.push({
      roundNum: round.roundNum,
      isFinal: round.isFinal,
      processItems: aggregateProcessItems(items),
      answer: round.answer.trim() || undefined,
    });
  }

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
