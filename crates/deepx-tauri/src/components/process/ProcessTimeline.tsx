import { createSignal, For } from "solid-js";
import type { ProcessItem } from "../../presentation/processAggregation";
import ProcessEventRow from "./ProcessEventRow";

export default function ProcessTimeline(props: { items: ProcessItem[] }) {
  const [expandedId, setExpandedId] = createSignal<string | null>(null);

  return (
    <div class="process-timeline" role="list">
      <For each={props.items} keyed={false}>
        {(item) => (
          <ProcessEventRow
            item={item()}
            expanded={() => expandedId() === item().id}
            onToggle={() => setExpandedId(current => current === item().id ? null : item().id)}
          />
        )}
      </For>
    </div>
  );
}
