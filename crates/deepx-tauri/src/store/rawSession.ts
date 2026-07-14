import type {
  AskMode,
  AskQuestion,
  PermissionRisk,
  RoundBlock,
  SkillInfo,
  ToolCallDef,
  ToolResultDef,
  UsageInfo,
} from "../lib/types";

export type TurnStatus = "running" | "waiting" | "completed" | "failed" | "cancelled";

export type RawProgressChunk = {
  stream: "stdout" | "stderr";
  seq: number;
  chunk: string;
};

export type RawProgress = { chunks: RawProgressChunk[] };

export type RawRound = {
  roundNum: number;
  isFinal: boolean;
  thinking: string;
  answer: string;
  blocks: RoundBlock[];
  toolCalls: ToolCallDef[];
  toolResults: Record<string, ToolResultDef>;
  progress: Record<string, RawProgress>;
};

export type InteractionRecord = {
  id: string;
  kind: "permission" | "ask" | "plan";
  resolution: string;
  at: number;
};

export type PendingInteraction =
  | {
      kind: "permission";
      id: string;
      toolName: string;
      reason: string;
      paths: string[];
      category: string;
      level: number;
      risk: PermissionRisk;
      consequence: string;
    }
  | {
      kind: "ask";
      id: string;
      turnId: string;
      roundNum: number;
      mode: AskMode;
      questions: AskQuestion[];
    }
  | { kind: "plan"; id: string };

export type RawTurn = {
  turnId: string;
  userText: string;
  status: TurnStatus;
  startedAt?: number;
  endedAt?: number;
  stopReason?: string;
  usage?: UsageInfo;
  rounds: RawRound[];
  interactions: InteractionRecord[];
};

export type RawSessionState = {
  seed: string;
  turns: RawTurn[];
  pendingInteraction: PendingInteraction | null;
  environment: {
    linesAdded: number;
    linesRemoved: number;
    filesCreated: number;
    filesDeleted: number;
    changedFiles: string[];
  };
  session: {
    ready: boolean;
    hasMore: boolean;
    totalTurns: number;
    tokensUsed: number;
    cacheHitPct: number;
    title?: string;
    model?: string;
    contextLimit: number;
  };
  skills: { available: SkillInfo[]; active: string[] };
  notices: Array<{ level: string; message: string; at: number }>;
  compact: { active: boolean; text: string };
};

export function emptyRawRound(roundNum: number): RawRound {
  return {
    roundNum,
    isFinal: false,
    thinking: "",
    answer: "",
    blocks: [],
    toolCalls: [],
    toolResults: {},
    progress: {},
  };
}
