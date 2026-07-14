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
import type { QueuedPermission } from "../store/permissionQueue";

interface ChatViewProps {
  chat: ReturnType<typeof import("../store/chat").createChatStore>;
  rawSession: () => RawSessionState | undefined;
  hasMore: boolean;
  onLoadMore: () => void;
  onSlashCommand: (cmd: SlashCommand) => void;
  permission?: () => QueuedPermission | null;
  onPermissionRespond?: (
    permission: QueuedPermission,
    approved: boolean,
    trustFolder: boolean,
  ) => Promise<void>;
}

export default function ChatView(props: ChatViewProps) {
  const chat = () => props.chat;
  const seed = () => chat().sessionInfo.seed;
  const [mode, setMode] = createSignal("plan");
  const [environmentOpen, setEnvironmentOpen] = createSignal(false);
  const [branch, setBranch] = createSignal("");
  const [showGitWorkspace, setShowGitWorkspace] = createSignal(false);
  const permission = () => props.permission?.() ?? null;
  const showCompactStatus = () => chat().isCompacting() || chat().compactResult() != null;
  const hasInteractions = () =>
    showCompactStatus() || permission() !== null || chat().askState().show;

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
        onToggleEnvironment={() => setEnvironmentOpen((value) => !value)}
        onOpenLocation={() => {
          if (chat().workspace()) void open(chat().workspace());
        }}
        onCompact={handleCompact}
      />
      <Show when={environmentOpen() && props.rawSession?.()}>
        {(raw) => (
          <EnvironmentPopover
            session={raw()}
            workspace={chat().workspace()}
            branch={branch()}
            onOpenDiff={() => setShowGitWorkspace(true)}
          />
        )}
      </Show>
      <Show when={props.rawSession()}>
        {(raw) => <ConversationTranscript turns={projectSession(raw())} />}
      </Show>
      <Show when={hasInteractions()}>
        <InteractionDock>
          <Show when={showCompactStatus()}>
            <CompactStatusRow
              active={chat().isCompacting()}
              status={chat().isCompacting() ? "active" : "complete"}
              text={chat().compactText()}
              turnsCompacted={chat().compactResult() ?? undefined}
            />
          </Show>
          <Show
            when={permission()}
            fallback={
              <Show when={chat().askState().show}>
                <AskUserPrompt
                  questions={chat().askState().questions}
                  onSubmit={(answers) => chat().submitAskAnswer(answers)}
                  onDismiss={() => void chat().dismissAsk()}
                />
              </Show>
            }
          >
            {(item) => (
              <PermissionPrompt
                request={item().request}
                onRespond={(approved, trustFolder) =>
                  props.onPermissionRespond?.(item(), approved, trustFolder)
                }
              />
            )}
          </Show>
        </InteractionDock>
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
