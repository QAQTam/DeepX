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
  stageAnswers: Array<{ roundNum: number; markdown: string }>;
  finalAnswer?: { markdown: string; streaming: boolean };
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
    if (interaction.kind === "permission") continue;
    items.push({
      kind: "interaction",
      id: interaction.id,
      label: interaction.kind,
      resolution: interaction.resolution,
    });
  }

  const answeredRounds = rawTurn.rounds.filter(round => round.answer.trim());
  const finalRound = answeredRounds[answeredRounds.length - 1];
  const stageAnswers = answeredRounds.slice(0, -1).map(round => ({
    roundNum: round.roundNum,
    markdown: round.answer,
  }));
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
    stageAnswers,
    finalAnswer: finalRound?.answer.trim()
      ? { markdown: finalRound.answer, streaming: rawTurn.status === "running" }
      : undefined,
  };
}
