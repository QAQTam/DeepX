import { createEffect, createSignal, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import type { SlashCommand } from "./SlashMenu";
import ConversationTranscript from "./conversation/ConversationTranscript";
import { projectSession } from "../presentation/useConversationView";
import type { RawSessionState } from "../store/rawSession";
import ThreadHeader from "./shell/ThreadHeader";
import EnvironmentPopover from "./shell/EnvironmentPopover";
import GitDiffPanel from "./GitDiffPanel";
import ComposerDock from "./composer/ComposerDock";
import { createFollowUpQueue } from "../store/followUpQueue";
import InteractionDock from "./interactions/InteractionDock";
import AskUserPrompt from "./interactions/AskUserPrompt";
import PermissionPrompt from "./interactions/PermissionPrompt";
import CompactStatusRow from "./interactions/CompactStatusRow";
import InteractionModal from "./interactions/InteractionModal";
import PlanReviewPanel from "./PlanReviewPanel";
import ContextPanel from "./ContextPanel";
import type { PermissionQueueProgress, QueuedPermission } from "../store/permissionQueue";

interface ChatViewProps {
  chat: ReturnType<typeof import("../store/chat").createChatStore>;
  rawSession: () => RawSessionState | undefined;
  hasMore: boolean;
  onLoadMore: () => void;
  onSlashCommand: (cmd: SlashCommand) => void;
  permission?: () => QueuedPermission | null;
  permissionProgress?: () => PermissionQueueProgress | null;
  onPermissionRespond?: (
    permission: QueuedPermission,
    approved: boolean,
    trustFolder: boolean,
  ) => Promise<void>;
  permissionLevel: number;
  onPermissionLevelChange: (level: number) => void | Promise<void>;
  onChangeWorkspace: () => void | Promise<void>;
  planReviewOpen?: () => boolean;
  planReviewCallId?: () => string;
  planReviewContent?: () => string;
  onPlanReviewRespond?: (approved: boolean, message?: string) => void;
  onPlanReviewClose?: () => void;
}

export default function ChatView(props: ChatViewProps) {
  const chat = () => props.chat;
  const seed = () => chat().sessionInfo.seed;
  const [mode, setMode] = createSignal("plan");
  const [environmentOpen, setEnvironmentOpen] = createSignal(false);
  const [statsOpen, setStatsOpen] = createSignal(false);
  const [branch, setBranch] = createSignal("");
  const [showGitWorkspace, setShowGitWorkspace] = createSignal(false);
  const permission = () => props.permission?.() ?? null;
  const showCompactStatus = () => chat().isCompacting() || chat().compactResult() != null;

  async function handleSetMode(m: string) {
    setMode(m);
    try {
      await invoke("cmd_set_mode", { seed: seed(), mode: m });
    } catch (e) {
      console.error("set_mode error:", e);
    }
  }

  async function handleSend(text: string, files: string[]) {
    try {
      chat().clearError();
      await invoke("cmd_send_message", { seed: seed(), text, files });
    } catch (e) {
      console.error("send_message error:", e);
    }
  }

  async function handleStop() {
    try {
      await invoke("cmd_cancel", { seed: seed() });
    } catch (e) {
      console.error("cancel error:", e);
    }
  }

  async function handleCompact() {
    try {
      await invoke("cmd_compact", { seed: seed() });
    } catch (e) {
      console.error(e);
    }
  }

  const followUps = createFollowUpQueue(seed(), handleSend);
  let wasStreaming = chat().isStreaming();
  createEffect(() => {
    const streaming = chat().isStreaming();
    if (wasStreaming && !streaming) {
      void followUps.drainAfterTurnEnd({
        hasPendingGate: !!props.rawSession?.()?.pendingInteraction,
      });
    }
    wasStreaming = streaming;
  });

  createEffect(() => {
    if (!environmentOpen()) return;
    invoke<string>("cmd_get_git_branch", { seed: seed() })
      .then(setBranch)
      .catch(() => setBranch(""));
  });

  return (
    <div class="chat-view">
      <ThreadHeader
        title={
          props.rawSession?.()?.session.title || seed().slice(0, 8)
        }
        environmentOpen={environmentOpen()}
        statsOpen={statsOpen()}
        onToggleEnvironment={() => setEnvironmentOpen((value) => !value)}
        onToggleStats={() => setStatsOpen((value) => !value)}
        onOpenLocation={() => {
          if (chat().workspace()) void open(chat().workspace());
        }}
        workspace={chat().workspace()}
        onChangeWorkspace={props.onChangeWorkspace}
        compacting={chat().isCompacting()}
        onCompact={handleCompact}
      />
      <Show when={environmentOpen() && props.rawSession?.()}>
        {(raw) => (
          <EnvironmentPopover
            session={raw()}
            workspace={chat().workspace()}
            branch={branch()}
            tasks={chat().tasks()}
            onOpenDiff={() => setShowGitWorkspace(true)}
            onTaskAction={(action, task) => void chat().submitTaskAction(action, task.id, task.subject, task.description)}
          />
        )}
      </Show>
      <Show when={statsOpen()}>
        <ContextPanel
          seed={seed()}
          metricHistory={chat().metricHistory()}
          contextLimit={chat().sessionInfo.context_limit ?? 200000}
          initialOpen={true}
        />
      </Show>
      <Show when={props.rawSession()}>
        {(raw) => <ConversationTranscript turns={projectSession(raw())} />}
      </Show>
      <Show when={showCompactStatus()}>
        <InteractionDock>
          <CompactStatusRow
            active={chat().isCompacting()}
            status={chat().isCompacting() ? "active" : "complete"}
            text={chat().compactText()}
            turnsCompacted={chat().compactResult() ?? undefined}
          />
        </InteractionDock>
      </Show>
      <Show
        when={permission()}
        fallback={
          <Show
            when={chat().askState().show}
            fallback={
              <Show when={props.planReviewOpen?.()}>
                <InteractionModal label="审核执行计划">
                  <PlanReviewPanel
                    seed={seed()}
                    callId={props.planReviewCallId?.() ?? ""}
                    planContent={props.planReviewContent?.() ?? ""}
                    onApprove={() => props.onPlanReviewRespond?.(true)}
                    onReject={(message) => props.onPlanReviewRespond?.(false, message)}
                    onClose={() => props.onPlanReviewClose?.()}
                  />
                </InteractionModal>
              </Show>
            }
          >
              <InteractionModal label="DeepX 需要你的回答">
                <AskUserPrompt
                  questions={chat().askState().questions}
                  onSubmit={(answers) => chat().submitAskAnswer(answers)}
                  onDismiss={() => void chat().dismissAsk()}
                />
              </InteractionModal>
          </Show>
        }
      >
        {(item) => (
          <InteractionModal label="DeepX 请求操作授权">
              <PermissionPrompt
                request={item().request}
                progress={props.permissionProgress?.()}
                onRespond={(approved, trustFolder) =>
                  props.onPermissionRespond?.(item(), approved, trustFolder)
                }
              />
          </InteractionModal>
        )}
      </Show>
      <ComposerDock
        onSend={handleSend}
        onStop={handleStop}
        isStreaming={chat().isStreaming}
        hasPendingGate={() => !!props.rawSession?.()?.pendingInteraction}
        queue={followUps}
        mode={mode()}
        onModeChange={handleSetMode}
        model={chat().sessionInfo.model}
        permissionLevel={props.permissionLevel}
        onPermissionLevelChange={props.onPermissionLevelChange}
      />

      {/* ── Git Diff Workspace Overlay ── */}
      <GitDiffPanel
        open={showGitWorkspace()}
        seed={seed()}
        onClose={() => setShowGitWorkspace(false)}
      />
    </div>
  );
}
