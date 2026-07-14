import { createSignal, For, Show } from "solid-js";
import type { ProcessItem } from "../../presentation/processAggregation";

const PREVIEW_LINES = 24;

function detailText(item: ProcessItem): string {
  switch (item.kind) {
    case "reasoning": return item.content;
    case "assistant_progress": return item.markdown;
    case "tool": {
      const progress = item.progress?.map(event =>
        event.stream === "stderr" ? `[stderr] ${event.chunk}` : event.chunk,
      ).join("") ?? "";
      return item.output ?? progress ?? item.argsJson ?? "";
    }
    case "interaction": return item.resolution;
    case "notice": return item.message;
    case "group": return "";
  }
}

export default function ProcessDetail(props: { item: ProcessItem }) {
  const [full, setFull] = createSignal(false);
  const lines = () => detailText(props.item).split("\n");
  const visible = () => full() ? lines() : lines().slice(0, PREVIEW_LINES);

  return (
    <div class="process-detail">
      <Show when={props.item.kind === "group"} fallback={
        <>
          <pre>{visible().join("\n")}</pre>
          <Show when={lines().length > PREVIEW_LINES}>
            <button type="button" class="process-show-full" onClick={() => setFull(value => !value)}>
              {full() ? "收起输出" : "显示完整输出"}
            </button>
          </Show>
        </>
      }>
        <ul class="process-group-children">
          <For each={props.item.kind === "group" ? props.item.children : []}>
            {(child) => <li>{child.kind === "tool" ? child.summary : child.kind}</li>}
          </For>
        </ul>
      </Show>
    </div>
  );
}
