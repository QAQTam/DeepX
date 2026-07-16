import { createEffect, createSignal, Match, onCleanup, Show, Switch, type Accessor } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import type { AskAnswer, TaskInfo } from "../lib/types";
import { projectSession } from "../presentation/useConversationView";
import type { PendingInteraction, RawSessionState } from "../store/rawSession";
import { createFollowUpQueue } from "../store/followUpQueue";
import {
  activeInteraction,
  canLoadMore,
  isSessionStreaming,
  sessionUsage,
} from "../store/sessionSelectors";
import type { SessionUiState } from "../store/sessionUiState";
import ComposerDock from "./composer/ComposerDock";
import ConversationTranscript from "./conversation/ConversationTranscript";
import GitDiffPanel from "./GitDiffPanel";
import AskUserPrompt from "./interactions/AskUserPrompt";
import CompactStatusRow from "./interactions/CompactStatusRow";
import InteractionDock from "./interactions/InteractionDock";
import InteractionModal from "./interactions/InteractionModal";
import PermissionPrompt from "./interactions/PermissionPrompt";
import PlanReviewPanel from "./PlanReviewPanel";
import ContextPanel from "./ContextPanel";
import EnvironmentPopover from "./shell/EnvironmentPopover";
import ThreadHeader from "./shell/ThreadHeader";

interface ChatViewProps {
  rawSession: Accessor<RawSessionState>;
  ui: SessionUiState;
  onLoadMore: () => void | Promise<void>;
  onAskSubmit: (
    item: Extract<PendingInteraction, { kind: "ask" }>,
    answers: AskAnswer[],
  ) => void | Promise<void>;
  onAskDismiss: (item: Extract<PendingInteraction, { kind: "ask" }>) => void | Promise<void>;
  onPermissionRespond: (
    item: Extract<PendingInteraction, { kind: "permission" }>,
    approved: boolean,
    trustFolder: boolean,
  ) => void | Promise<void>;
  onPlanRespond: (
    item: Extract<PendingInteraction, { kind: "plan" }>,
    approved: boolean,
    message?: string,
  ) => void | Promise<void>;
  onTaskAction: (action: "cancel" | "delete" | "ask", task: TaskInfo) => void | Promise<void>;
  onUndo: () => void | Promise<void>;
  permissionLevel: number;
  onPermissionLevelChange: (level: number) => void | Promise<void>;
  onChangeWorkspace: () => void | Promise<void>;
}

export default function ChatView(props: ChatViewProps) {
  const session = () => props.rawSession();
  const seed = () => session().seed;
  const interaction = () => activeInteraction(session());
  const permissionInteraction = () => {
    const item = interaction();
    return item?.kind === "permission" ? item : null;
  };
  const askInteraction = () => {
    const item = interaction();
    return item?.kind === "ask" ? item : null;
  };
  const planInteraction = () => {
    const item = interaction();
    return item?.kind === "plan" ? item : null;
  };
  const streaming = () => isSessionStreaming(session());
  const usage = () => sessionUsage(session());
  const [mode, setMode] = createSignal("plan");
  const [environmentOpen, setEnvironmentOpen] = createSignal(false);
  const [statsOpen, setStatsOpen] = createSignal(false);
  const [branch, setBranch] = createSignal("");
  const [showGitWorkspace, setShowGitWorkspace] = createSignal(false);
  const [compactCompleteVisible, setCompactCompleteVisible] = createSignal(
    session().compact.completionRevision > 0,
  );
  let compactRevision = 0;
  let compactTimer: ReturnType<typeof setTimeout> | undefined;

  createEffect(() => {
    const revision = session().compact.completionRevision;
    if (revision > compactRevision) {
      setCompactCompleteVisible(true);
      if (compactTimer) clearTimeout(compactTimer);
      compactTimer = setTimeout(() => setCompactCompleteVisible(false), 4_000);
    }
    compactRevision = revision;
  });
  onCleanup(() => { if (compactTimer) clearTimeout(compactTimer); });

  async function handleSetMode(nextMode: string) {
    setMode(nextMode);
    try { await invoke("cmd_set_mode", { seed: seed(), mode: nextMode }); }
    catch (error) { console.error("set_mode error:", error); }
  }

  async function handleSend(text: string, files: string[]) {
    try { await invoke("cmd_send_message", { seed: seed(), text, files }); }
    catch (error) { console.error("send_message error:", error); }
  }

  async function handleStop() {
    try { await invoke("cmd_cancel", { seed: seed() }); }
    catch (error) { console.error("cancel error:", error); }
  }

  async function handleCompact() {
    try { await invoke("cmd_compact", { seed: seed() }); }
    catch (error) { console.error("compact error:", error); }
  }

  const followUps = createFollowUpQueue(seed(), handleSend);
  let wasStreaming = streaming();
  createEffect(() => {
    const active = streaming();
    if (wasStreaming && !active) {
      void followUps.drainAfterTurnEnd({ hasPendingGate: activeInteraction(session()) !== null });
    }
    wasStreaming = active;
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
        title={session().session.title || seed().slice(0, 8)}
        environmentOpen={environmentOpen()}
        statsOpen={statsOpen()}
        onToggleEnvironment={() => setEnvironmentOpen(value => !value)}
        onToggleStats={() => setStatsOpen(value => !value)}
        onOpenLocation={() => { if (props.ui.workspace()) void open(props.ui.workspace()); }}
        workspace={props.ui.workspace()}
        onChangeWorkspace={props.onChangeWorkspace}
        compacting={session().compact.active}
        onCompact={handleCompact}
        undoDisabled={session().turns.length === 0 || streaming()}
        onUndo={() => void props.onUndo()}
      />
      <Show when={environmentOpen()}>
        <EnvironmentPopover
          session={session()}
          workspace={props.ui.workspace()}
          branch={branch()}
          tasks={session().dashboard.tasks}
          onOpenDiff={() => setShowGitWorkspace(true)}
          onTaskAction={(action, task) => void props.onTaskAction(action, task)}
        />
      </Show>
      <Show when={statsOpen()}>
        <ContextPanel
          seed={seed()}
          metricHistory={session().telemetry}
          contextLimit={usage().contextLimit || 200000}
          initialOpen={true}
        />
      </Show>
      <ConversationTranscript
        turns={projectSession(session())}
        hasMore={canLoadMore(session())}
        onLoadMore={props.onLoadMore}
      />
      <Show when={session().compact.active || compactCompleteVisible()}>
        <InteractionDock>
          <CompactStatusRow
            active={session().compact.active}
            status={session().compact.active ? "active" : "complete"}
            text={session().compact.text}
            turnsCompacted={session().compact.turnsCompacted ?? undefined}
          />
        </InteractionDock>
      </Show>
      <Switch>
        <Match when={permissionInteraction()}>
          {item => <InteractionModal label="DeepX 请求操作授权">
            <PermissionPrompt
              request={{
                tool_call_id: item().id,
                tool_name: item().toolName,
                reason: item().reason,
                paths: item().paths,
                category: item().category,
                level: item().level,
                risk: item().risk,
                consequence: item().consequence,
              }}
              onRespond={(approved, trust) => void props.onPermissionRespond(item(), approved, trust)}
            />
          </InteractionModal>}
        </Match>
        <Match when={askInteraction()}>
          {item => <InteractionModal label="DeepX 需要你的回答">
            <AskUserPrompt
              questions={item().questions}
              onSubmit={answers => void props.onAskSubmit(item(), answers)}
              onDismiss={() => void props.onAskDismiss(item())}
            />
          </InteractionModal>}
        </Match>
        <Match when={planInteraction()}>
          {item => <InteractionModal label="审核执行计划">
            <PlanReviewPanel
              planContent={item().content}
              onApprove={() => props.onPlanRespond(item(), true)}
              onReject={message => props.onPlanRespond(item(), false, message)}
            />
          </InteractionModal>}
        </Match>
      </Switch>
      <ComposerDock
        onSend={handleSend}
        onStop={handleStop}
        isStreaming={streaming}
        hasPendingGate={() => activeInteraction(session()) !== null}
        queue={followUps}
        mode={mode()}
        onModeChange={handleSetMode}
        model={usage().model}
        contextTokens={usage().contextTokens}
        contextLimit={usage().contextLimit}
        permissionLevel={props.permissionLevel}
        onPermissionLevelChange={props.onPermissionLevelChange}
      />
      <GitDiffPanel
        open={showGitWorkspace()}
        seed={seed()}
        onClose={() => setShowGitWorkspace(false)}
      />
    </div>
  );
}
