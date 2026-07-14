import { For, Show } from "solid-js";
import type { TurnViewModel } from "../../presentation/turnProjection";
import ProcessDisclosure from "../process/ProcessDisclosure";
import ProcessTimeline from "../process/ProcessTimeline";
import AssistantAnswer from "./AssistantAnswer";
import UserPromptBubble from "./UserPromptBubble";

export default function TurnGroup(props: { turn: TurnViewModel }) {
  return <article class="conversation-turn" data-turn={props.turn.turnId}>
    <UserPromptBubble text={props.turn.userPrompt} />
    <For each={props.turn.stageAnswers}>
      {(answer) => <AssistantAnswer markdown={answer.markdown} stage />}
    </For>
    <div data-part="process">
      <ProcessDisclosure process={props.turn.process}>
        <ProcessTimeline items={props.turn.process.items} />
      </ProcessDisclosure>
    </div>
    <Show when={props.turn.finalAnswer}>
      {(answer) => <AssistantAnswer markdown={answer().markdown} streaming={answer().streaming} />}
    </Show>
  </article>;
}
