import type { ProcessItem } from "../../presentation/processAggregation";
import ProcessDetail from "./ProcessDetail";

function label(item: ProcessItem): string {
  switch (item.kind) {
    case "reasoning": return "分析了实现路径";
    case "assistant_progress": return "形成了阶段结论";
    case "tool": return item.summary || item.toolName;
    case "group": return item.label;
    case "interaction": return `${item.label}: ${item.resolution}`;
    case "notice": return item.message;
  }
}

export default function ProcessEventRow(props: {
  item: ProcessItem;
  expanded: () => boolean;
  onToggle: () => void;
}) {
  const failed = () => props.item.kind === "tool" && props.item.success === false;

  return (
    <div
      class="process-event-row"
      classList={{ "is-failed": failed() }}
      data-process-row
      data-kind={props.item.kind}
      aria-expanded={props.expanded()}
      role="listitem"
    >
      <button type="button" class="process-event-trigger" onClick={props.onToggle}>
        <span class="process-event-icon" aria-hidden="true">
          {failed() ? "!" : props.item.kind === "tool" ? "›_" : "·"}
        </span>
        <span class="process-event-label">{label(props.item)}</span>
        <span class="process-event-expand" aria-hidden="true">{props.expanded() ? "−" : "+"}</span>
      </button>
      {props.expanded() && <ProcessDetail item={props.item} />}
    </div>
  );
}
