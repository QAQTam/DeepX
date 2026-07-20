import { For, Show } from "solid-js";
import type { RoundRenderEntry, TurnViewModel } from "../../presentation/turnProjection";
import ProcessDisclosure from "../process/ProcessDisclosure";
import ProcessTimeline from "../process/ProcessTimeline";
import AssistantAnswer from "./AssistantAnswer";
import UserPromptBubble from "./UserPromptBubble";

export type ProcessStatus = "running" | "waiting" | "completed" | "failed" | "cancelled";

type AssistantEntry = Extract<RoundRenderEntry, { kind: "assistant" }>;

function assistantEntry(entry: RoundRenderEntry): AssistantEntry | undefined {
  return entry.kind === "assistant" ? entry : undefined;
}

export default function TurnGroup(props: { turn: TurnViewModel }) {
  const status = () => props.turn.status as ProcessStatus;
  const activity = () => props.turn.rounds.flatMap(round =>
    round.entries.flatMap(entry => entry.kind === "process" ? entry.items : []),
  );
  const toolCount = () => activity().filter(item => item.kind === "tool" || item.kind === "group")
    .reduce((count, item) => count + (item.kind === "group" ? item.children.length : 1), 0);
  const activitySummary = () => {
    const count = toolCount();
    return count > 0 ? `完成 ${count} 项操作` : "处理过程";
  };

  return (
    <article class="conversation-turn" data-turn={props.turn.turnId}>
      <UserPromptBubble text={props.turn.userPrompt} />

      <Show when={activity().length > 0}>
        <div data-part="process">
          <ProcessDisclosure
            status={status()}
            defaultOpen={false}
            summary={activitySummary()}
            tokensPerSec={status() === "completed" ? props.turn.tokensPerSec : undefined}
          >
            <ProcessTimeline items={activity()} />
          </ProcessDisclosure>
        </div>
      </Show>

      <For each={props.turn.rounds} keyed={false}>
        {(round) => (
          <For each={round().entries} keyed={false}>
            {(entry) => (
              <Show
                when={assistantEntry(entry())}
                fallback={null}
              >
                {(assistant) => (
                  <AssistantAnswer
                    markdown={assistant().markdown}
                    streaming={assistant().streaming}
                  />
                )}
              </Show>
            )}
          </For>
        )}
      </For>
    </article>
  );
}
