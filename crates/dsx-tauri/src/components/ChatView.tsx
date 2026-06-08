import { invoke } from "@tauri-apps/api/core";
import MessageList from "./MessageList";
import InputBar from "./InputBar";

interface ChatViewProps { chat: ReturnType<typeof import("../store/chat").createChatStore>; }

export default function ChatView(props: ChatViewProps) {
  const { chat } = props;

  async function handleSend(text: string) {
    try {
      await invoke("cmd_send_message", { text });
    } catch (e) {
      console.error("send_message error:", e);
    }
  }

  async function handleStop() {
    try {
      await invoke("cmd_cancel");
    } catch (e) {
      console.error("cancel error:", e);
    }
  }

  return (
    <div class="chat-view">
      <MessageList turns={chat.turns} isStreaming={chat.isStreaming} />
      <InputBar
        onSend={handleSend}
        onStop={handleStop}
        isStreaming={chat.isStreaming}
        disabled={chat.inputDisabled()}
      />
    </div>
  );
}
