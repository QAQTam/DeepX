import { createSignal, For, Show } from "solid-js";
import type { ChangeReviewFile, TurnViewModel } from "../../presentation/turnProjection";
import type { ProcessItem } from "../../presentation/processAggregation";
import ProcessTimeline from "../process/ProcessTimeline";
import AssistantAnswer from "./AssistantAnswer";
import UserPromptBubble from "./UserPromptBubble";
import { useI18n } from "../../i18n";

export type ProcessStatus = "running" | "waiting" | "completed" | "failed" | "cancelled";

/** Session-level preference: once the user expands a timeline, default all timelines to expanded. */
const [preferExpanded, setPreferExpanded] = createSignal(false);

type GroupedEntry =
  | { kind: "process-group"; items: ProcessItem[] }
  | { kind: "assistant"; markdown: string; streaming: boolean };

/** Flatten and merge consecutive process entries into groups. */
function mergeProcessEntries(
  rounds: TurnViewModel["rounds"],
): GroupedEntry[][] {
  return rounds.map(round => {
    const result: GroupedEntry[] = [];
    let buffer: ProcessItem[] = [];
    const flush = () => {
      if (buffer.length > 0) {
        result.push({ kind: "process-group", items: buffer });
        buffer = [];
      }
    };
    for (const entry of round.entries) {
      if (entry.kind === "process") {
        buffer.push(...entry.items);
      } else {
        flush();
        result.push({ kind: "assistant", markdown: entry.markdown, streaming: entry.streaming });
      }
    }
    flush();
    return result;
  });
}

export default function TurnGroup(props: { turn: TurnViewModel; onReviewChanges?: (changes: ChangeReviewFile[]) => void }) {
  const { t } = useI18n();
  const status = () => props.turn.status as ProcessStatus;
  const changes = () => props.turn.changes ?? [];
  const changeTotals = () => changes().reduce(
    (sum, change) => ({ added: sum.added + change.added, removed: sum.removed + change.removed }),
    { added: 0, removed: 0 },
  );
  const grouped = () => mergeProcessEntries(props.turn.rounds);

  const onExpand = () => setPreferExpanded(true);

  return (
    <article class="conversation-turn" data-turn={props.turn.turnId}>
      <UserPromptBubble text={props.turn.userPrompt} />

      <For each={grouped()} keyed={false}>
        {(round) => (
          <For each={round()} keyed={false}>
            {(entry) => {
              const e = entry();
              if (e.kind === "process-group") {
                return (
                  <div data-part="process">
                    <ProcessTimeline
                      items={e.items}
                      expandable={true}
                      defaultExpanded={preferExpanded()}
                      onExpand={onExpand}
                    />
                  </div>
                );
              }
              if (e.kind === "assistant") {
                return (
                  <AssistantAnswer
                    markdown={e.markdown}
                    streaming={e.streaming}
                  />
                );
              }
              return null;
            }}
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
