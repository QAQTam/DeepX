import { For, Show, Switch, Match, createMemo } from "solid-js";
import MarkdownBody from "./MarkdownBody";
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
          {isUser ? t().message.you : t().message.assistant}
          <Show when={isUser && props.turnId && props.onUndo}>
            <span class="msg-undo" onClick={() => props.onUndo!(props.turnId!)} title={t().message.undo}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M3 10h10a5 5 0 0 1 0 10H11" /><path d="M7 6l-4 4 4 4" />
              </svg>
            </span>
          </Show>
        </div>
        <Show when={props.text}>
          <div class="bubble-user">{props.text}</div>
        </Show>
        <Show when={props.rounds && props.rounds.length > 0}>
          <div class="msg-rounds">
            <For each={props.rounds}>
              {(round) => {
                // Merge tool results into tool calls / blocks so <For> re-renders
                // when toolResults change independently of toolCalls/blocks.
                const mergedToolCalls = createMemo(() =>
                  round.toolCalls.map((tc) => ({
                    call: tc,
                    result: round.toolResults.find((x) => x.tool_call_id === tc.id),
                    streamOutput: round.toolResults.find((x) => x.tool_call_id === tc.id + "_stream")?.output,
                  }))
                );
                const mergedBlocks = createMemo(() =>
                  round.blocks.map((block) => {
                    if (block.type !== "tool" || !block.card) return block;
                    const res = round.toolResults.find((x) => x.tool_call_id === block.card!.id);
                    const streamOut = round.toolResults.find((x) => x.tool_call_id === block.card!.id + "_stream")?.output;
                    return { ...block, card: { ...block.card, _result: res, _streamOutput: streamOut } };
                  })
                );
                return (
                  <div class={`msg-round ${props.status === "streaming" ? "streaming" : ""}`}>
                    <Show
                      when={round.blocks && round.blocks.length > 0}
                      fallback={
                        <>
                          <Show when={round.thinking}><ThinkingBlock content={round.thinking!} streaming={props.status === "streaming"} /></Show>
                          <Show when={round.answer}><MarkdownBody class="md-body bubble-ai" content={round.answer!} final={!!(round.blocks && round.blocks.length > 0)} /></Show>
                          <For each={mergedToolCalls()}>
                            {(item) => <ToolCallCard call={item.call} result={item.result} streamingOutput={item.streamOutput} />}
                          </For>
                        </>
                      }
                    >
                      <For each={mergedBlocks()}>
                        {(block: any) => (
                          <Switch>
                            <Match when={block.type === "reasoning"}>
                              <ThinkingBlock content={block.content!} />
                            </Match>
                            <Match when={block.type === "text"}>
                              <MarkdownBody class="md-body bubble-ai" content={block.content!} final={true} />
                            </Match>
                            <Match when={block.type === "tool"}>
                              <ToolCallCard call={block.card!} result={block.card._result} streamingOutput={block.card._streamOutput} />
                            </Match>
                          </Switch>
                        )}
                      </For>
                    </Show>
                  </div>
                );
              }}
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
