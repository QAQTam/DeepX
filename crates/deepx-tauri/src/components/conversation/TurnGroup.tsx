import { Index, Show } from "solid-js";
import type { RoundRenderEntry, TurnViewModel } from "../../presentation/turnProjection";
import ProcessDisclosure from "../process/ProcessDisclosure";
import ProcessTimeline from "../process/ProcessTimeline";
import AssistantAnswer from "./AssistantAnswer";
import UserPromptBubble from "./UserPromptBubble";

export type ProcessStatus = "running" | "waiting" | "completed" | "failed" | "cancelled";

type AssistantEntry = Extract<RoundRenderEntry, { kind: "assistant" }>;
type ProcessEntry = Extract<RoundRenderEntry, { kind: "process" }>;

function assistantEntry(entry: RoundRenderEntry): AssistantEntry | undefined {
  return entry.kind === "assistant" ? entry : undefined;
}

export default function TurnGroup(props: { turn: TurnViewModel }) {
  const status = () => props.turn.status as ProcessStatus;

  return (
    <article class="conversation-turn" data-turn={props.turn.turnId}>
      <UserPromptBubble text={props.turn.userPrompt} />

      <Index each={props.turn.rounds}>
        {(round) => (
          <Index each={round().entries}>
            {(entry) => (
              <Show
                when={assistantEntry(entry())}
                fallback={
                  <div data-part="process">
                    <ProcessDisclosure
                      status={status()}
                      defaultOpen={false}
                      tokensPerSec={
                        round().isFinal && status() === "completed"
                          ? props.turn.tokensPerSec
                          : undefined
                      }
                    >
                      <ProcessTimeline items={(entry() as ProcessEntry).items} />
                    </ProcessDisclosure>
                  </div>
                }
              >
                {(assistant) => (
                  <AssistantAnswer
                    markdown={assistant().markdown}
                    streaming={assistant().streaming}
                  />
                )}
              </Show>
            )}
          </Index>
        )}
      </Index>
    </article>
  );
}
