import type { RawTurn } from "../store/rawSession";
import { aggregateProcessItems, type ProcessItem } from "./processAggregation";

export type TurnViewModel = {
  turnId: string;
  userPrompt: string;
  process: {
    status: RawTurn["status"];
    elapsedMs?: number;
    items: ProcessItem[];
  };
  finalAnswer?: { markdown: string };
};

export function toolFamily(name: string): string {
  if (["read", "list", "search", "diff", "explore_scan"].includes(name)) return "read";
  if (["write", "edit", "edit_block", "delete"].includes(name)) return "write";
  if (["web", "web_search", "web_fetch"].includes(name)) return "web";
  if (["exec", "exec_run", "spawn_subagent"].includes(name)) return "exec";
  return "tool";
}

export function projectTurn(rawTurn: RawTurn): TurnViewModel {
  const items: ProcessItem[] = [];

  for (const round of rawTurn.rounds) {
    if (round.thinking.trim()) {
      items.push({
        kind: "reasoning",
        id: `${rawTurn.turnId}-reasoning-${round.roundNum}`,
        content: round.thinking,
      });
    }
    if (!round.isFinal && round.answer.trim()) {
      items.push({
        kind: "assistant_progress",
        id: `${rawTurn.turnId}-progress-${round.roundNum}`,
        markdown: round.answer,
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
  }

  for (const interaction of rawTurn.interactions) {
    items.push({
      kind: "interaction",
      id: interaction.id,
      label: interaction.kind,
      resolution: interaction.resolution,
    });
  }

  const finalRound = [...rawTurn.rounds].reverse().find(round => round.isFinal);
  const elapsedMs = rawTurn.startedAt !== undefined && rawTurn.endedAt !== undefined
    ? Math.max(0, rawTurn.endedAt - rawTurn.startedAt)
    : undefined;

  return {
    turnId: rawTurn.turnId,
    userPrompt: rawTurn.userText,
    process: {
      status: rawTurn.status,
      elapsedMs,
      items: aggregateProcessItems(items),
    },
    finalAnswer: finalRound?.answer.trim()
      ? { markdown: finalRound.answer }
      : undefined,
  };
}
