import { createEffect, createSignal, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import type { SlashCommand } from "./SlashMenu";
import AskDialog from "./AskDialog";
import AskForm from "./AskForm";
import ConversationTranscript from "./conversation/ConversationTranscript";
import { projectSession } from "../presentation/useConversationView";
import type { RawSessionState } from "../store/rawSession";
import ThreadHeader from "./shell/ThreadHeader";
import EnvironmentPopover from "./shell/EnvironmentPopover";
import GitDiffPanel from "./GitDiffPanel";
import ComposerDock from "./composer/ComposerDock";
import { createFollowUpQueue } from "../store/followUpQueue";

interface ChatViewProps {
  chat: ReturnType<typeof import("../store/chat").createChatStore>;
  rawSession: () => RawSessionState | undefined;
  hasMore: boolean;
  onLoadMore: () => void;
  onSlashCommand: (cmd: SlashCommand) => void;
}

export default function ChatView(props: ChatViewProps) {
  const chat = () => props.chat;
  const seed = () => chat().sessionInfo.seed;
  const [mode, setMode] = createSignal("plan");
  const [environmentOpen, setEnvironmentOpen] = createSignal(false);
  const [branch, setBranch] = createSignal("");
  const [showGitWorkspace, setShowGitWorkspace] = createSignal(false);

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
      <AskDialog
        state={chat().askState}
        onSubmit={(a) => chat().submitAskAnswer(a)}
        onDismiss={() => chat().dismissAsk()}
      />
      <AskForm
        state={chat().askState}
        onSubmit={(a) => chat().submitAskAnswer(a)}
        onDismiss={() => chat().dismissAsk()}
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
