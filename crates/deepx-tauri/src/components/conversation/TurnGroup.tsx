import { Index, Show } from "solid-js";
import type { TurnViewModel } from "../../presentation/turnProjection";
import ProcessDisclosure from "../process/ProcessDisclosure";
import ProcessTimeline from "../process/ProcessTimeline";
import AssistantAnswer from "./AssistantAnswer";
import UserPromptBubble from "./UserPromptBubble";

export type ProcessStatus = "running" | "waiting" | "completed" | "failed" | "cancelled";

export default function TurnGroup(props: { turn: TurnViewModel }) {
  const status = () => props.turn.status as ProcessStatus;
  return (
    <article class="conversation-turn" data-turn={props.turn.turnId}>
      <UserPromptBubble text={props.turn.userPrompt} />

      <Index each={props.turn.rounds}>
        {(round) => {
          const hasItems = round().processItems.length > 0;
          const defaultOpen =
            !round().answer ||
            (round().isFinal && status() !== "completed");

          return (
            <>
              <Show when={hasItems}>
                <div data-part="process">
                  <ProcessDisclosure
                    status={status()}
                    defaultOpen={defaultOpen}
                    tokensPerSec={
                      round().isFinal && status() === "completed"
                        ? props.turn.tokensPerSec
                        : undefined
                    }
                  >
                    <ProcessTimeline items={round().processItems} />
                  </ProcessDisclosure>
                </div>
              </Show>
              <Show when={round().answer}>
                {(answer) => (
                  <AssistantAnswer
                    markdown={answer()}
                    stage={!round().isFinal}
                    streaming={round().isFinal && status() !== "completed"}
                  />
                )}
              </Show>
            </>
          );
        }}
      </Index>
    </article>
  );
}
