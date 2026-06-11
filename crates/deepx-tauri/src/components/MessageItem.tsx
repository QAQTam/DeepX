import { For, Show, Switch, Match } from "solid-js";
import ThinkingBlock from "./ThinkingBlock";
import ToolCallCard from "./ToolCallCard";
import type { Round } from "../store/chat";
import { useI18n } from "../i18n";

interface MessageItemProps {
  role: "user" | "assistant";
  text?: string;
  rounds?: Round[];
  status?: "streaming" | "complete";
  turnId?: string;
  onUndo?: (turnId: string) => void;
}

export default function MessageItem(props: MessageItemProps) {
  const { t } = useI18n();
  const isUser = props.role === "user";
  return (
    <div class="msg-item">
      <div class={`msg-avatar ${props.role}`}>{isUser ? "U" : "X"}</div>
      <div class="msg-body">
        <div class="msg-role">
          {isUser ? "You" : "DeepX"}
          <Show when={isUser && props.turnId && props.onUndo}>
            <span class="msg-undo" onClick={() => props.onUndo!(props.turnId!)} title="Undo from here">
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M3 10h10a5 5 0 0 1 0 10H11" /><path d="M7 6l-4 4 4 4" />
              </svg>
            </span>
          </Show>
        </div>
        <Show when={props.text}>
          <div class="msg-text">{props.text}</div>
        </Show>
        <Show when={props.rounds && props.rounds.length > 0}>
          <div class="msg-rounds">
            <For each={props.rounds}>
              {(round) => (
                <div class={`msg-round ${props.status === "streaming" ? "streaming" : ""}`}>
                  <Show when={round.blocks && round.blocks.length > 0}
                    fallback={
                      <>
                        <Show when={round.thinking}><ThinkingBlock content={round.thinking!} /></Show>
                        <Show when={round.answer}><div class="msg-text">{round.answer}</div></Show>
                        <For each={round.toolCalls}>{(tc) => {
                          const r = round.toolResults.find((x) => x.tool_call_id === tc.id);
                          return <ToolCallCard call={tc} result={r} />;
                        }}</For>
                      </>
                    }
                  >
                    <For each={round.blocks}>
                      {(block) => (
                        <Switch>
                          <Match when={block.type === "reasoning"}>
                            <ThinkingBlock content={block.content!} />
                          </Match>
                          <Match when={block.type === "text"}>
                            <div class="msg-text">{block.content!}</div>
                          </Match>
                          <Match when={block.type === "tool"}>
                            <ToolCallCard call={block.card!} result={round.toolResults.find((x) => x.tool_call_id === block.card!.id)} />
                          </Match>
                        </Switch>
                      )}
                    </For>
                  </Show>
                </div>
              )}
            </For>
          </div>
        </Show>
        <Show when={props.status === "streaming" && (!props.rounds || props.rounds.length === 0)}>
          <div class="stream-indicator">
            <div class="stream-dot" /><div class="stream-dot" /><div class="stream-dot" />
            <span>{t().chat.thinking}</span>
          </div>
        </Show>
      </div>
    </div>
  );
}
