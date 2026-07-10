import { createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import MessageList from "./MessageList";
import InputBar from "./InputBar";
import type { SlashCommand } from "./SlashMenu";
import InfoBar from "./InfoBar";
import AskDialog from "./AskDialog";

interface ChatViewProps { chat: ReturnType<typeof import("../store/chat").createChatStore>; hasMore: boolean; onLoadMore: () => void; onSlashCommand: (cmd: SlashCommand) => void; }

export default function ChatView(props: ChatViewProps) {
  const chat = () => props.chat;
  const seed = () => chat().sessionInfo.seed;
  const [mode, setMode] = createSignal("plan");

  async function handleSetMode(m: string) {
    setMode(m);
    try { await invoke("cmd_set_mode", { seed: seed(), mode: m }); }
    catch (e) { console.error("set_mode error:", e); }
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
    } catch (e) { console.error(e); }
  }

  return (
    <div class="chat-view">
      <InfoBar
        model={chat().sessionInfo.model}
        seed={chat().sessionInfo.seed}
        context_tokens={chat().sessionInfo.context_tokens}
        context_limit={chat().sessionInfo.context_limit}
        prompt_cache_hit={chat().sessionInfo.prompt_cache_hit}
        metricHistory={chat().metricHistory()}
        prompt_cache_miss={chat().sessionInfo.prompt_cache_miss}
        isStreaming={chat().isStreaming()}
        error={chat().error()}
        onDismissError={() => chat().clearError()}
        isCompacting={chat().isCompacting}
        compactResult={chat().compactResult}
        onCompact={handleCompact}
      />
      <MessageList turns={chat().turns} isStreaming={chat().isStreaming} onUndo={(id) => chat().undoTurn(id)} hasMore={props.hasMore} onLoadMore={props.onLoadMore} />
      <InputBar
        onSend={handleSend}
        onStop={handleStop}
        isStreaming={chat().isStreaming}
        disabled={chat().inputDisabled()}
        restoreText={chat().restoreText}
        mode={mode()}
        onModeChange={handleSetMode}
        onSlashCommand={props.onSlashCommand}
      />
      <AskDialog
        state={chat().askState}
        onSubmit={(a) => chat().submitAskAnswer(a)}
        onDismiss={() => chat().dismissAsk()}
      />
    </div>
  );
}
