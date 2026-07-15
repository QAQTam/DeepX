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
        {(round, index) => {
          const hasItems = () => round().processItems.length > 0;
          const isLiveRound = () =>
            status() === "running" && index === props.turn.rounds.length - 1;
          const isStage = () => !round().isFinal && !isLiveRound();
          const defaultOpen = () =>
            !round().answer || isLiveRound() || status() === "waiting";

          return (
            <>
              <Show when={hasItems()}>
                <div data-part="process">
                  <ProcessDisclosure
                    status={status()}
                    defaultOpen={defaultOpen()}
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
                    stage={isStage()}
                    streaming={isLiveRound()}
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
