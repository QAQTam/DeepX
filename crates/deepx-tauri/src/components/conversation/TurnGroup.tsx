import { For, Show } from "solid-js";
import type { ChangeReviewFile, RoundRenderEntry, TurnViewModel } from "../../presentation/turnProjection";
import ProcessDisclosure from "../process/ProcessDisclosure";
import ProcessTimeline from "../process/ProcessTimeline";
import AssistantAnswer from "./AssistantAnswer";
import UserPromptBubble from "./UserPromptBubble";
import { useI18n } from "../../i18n";

export type ProcessStatus = "running" | "waiting" | "completed" | "failed" | "cancelled";

type AssistantEntry = Extract<RoundRenderEntry, { kind: "assistant" }>;

function assistantEntry(entry: RoundRenderEntry): AssistantEntry | undefined {
  return entry.kind === "assistant" ? entry : undefined;
}

export default function TurnGroup(props: { turn: TurnViewModel; onReviewChanges?: (changes: ChangeReviewFile[]) => void }) {
  const { t } = useI18n();
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
  const changes = () => props.turn.changes ?? [];
  const changeTotals = () => changes().reduce(
    (sum, change) => ({ added: sum.added + change.added, removed: sum.removed + change.removed }),
    { added: 0, removed: 0 },
  );

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

      <Show when={status() === "completed" && changes().length > 0}>
        <div class="turn-change-receipt" data-part="turn-change-receipt">
          <span class="turn-change-receipt-files">{t().review.changedFiles.replace("{n}", String(changes().length))}</span>
          <Show when={changeTotals().added > 0}><span class="turn-change-add">+{changeTotals().added}</span></Show>
          <Show when={changeTotals().removed > 0}><span class="turn-change-del">-{changeTotals().removed}</span></Show>
          <Show when={props.onReviewChanges}>
            <button type="button" class="turn-change-review" onClick={() => props.onReviewChanges?.(changes())}>{t().review.reviewChanges}</button>
          </Show>
        </div>
      </Show>
    </article>
  );
}
