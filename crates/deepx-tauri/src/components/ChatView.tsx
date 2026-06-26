import { invoke } from "@tauri-apps/api/core";
import MessageList from "./MessageList";
import InputBar from "./InputBar";
import InfoBar from "./InfoBar";
import AskDialog from "./AskDialog";

interface ChatViewProps { chat: ReturnType<typeof import("../store/chat").createChatStore>; hasMore: boolean; onLoadMore: () => void; }

export default function ChatView(props: ChatViewProps) {
  // Use props.chat directly — SolidJS reactivity depends on prop access, not destructuring.
  const chat = () => props.chat;
  const seed = () => chat().sessionInfo.seed;

  async function handleSend(text: string) {
    try {
      chat().clearError();
      await invoke("cmd_send_message", { seed: seed(), text });
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
        contextTokens={chat().sessionInfo.contextTokens}
        contextLimit={chat().sessionInfo.contextLimit}
        promptCacheHit={chat().sessionInfo.promptCacheHit}
        promptCacheMiss={chat().sessionInfo.promptCacheMiss}
        isStreaming={chat().isStreaming()}
        error={chat().error()}
        onDismissError={() => chat().clearError()}
        isCompacting={chat().isCompacting}
        codeDeltas={chat().codeDeltas}
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
      />
      <AskDialog
        state={chat().askState}
        onSubmit={(a) => chat().submitAskAnswer(a)}
        onDismiss={() => chat().dismissAsk()}
      />
    </div>
  );
}
