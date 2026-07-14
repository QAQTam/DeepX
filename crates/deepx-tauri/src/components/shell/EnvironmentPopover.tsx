import { For, Show } from "solid-js";
import type { RawSessionState } from "../../store/rawSession";

export default function EnvironmentPopover(props: {
  session: RawSessionState;
  workspace: string;
  branch?: string;
  onOpenDiff?: () => void;
}) {
  return (
    <aside class="environment-popover" data-environment-popover>
      <div class="environment-heading">环境信息</div>
      <div
        class="environment-row environment-row-clickable"
        onClick={props.onOpenDiff}
        role="button"
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") props.onOpenDiff?.();
        }}
      >
        <span>变更</span>
        <span>
          <b class="added">+{props.session.environment.linesAdded}</b>{" "}
          <b class="removed">-{props.session.environment.linesRemoved}</b>
        </span>
      </div>
      <div class="environment-row">
        <span>本地</span>
        <span>{props.workspace || "未选择工作区"}</span>
      </div>
      <Show when={props.branch}>
        <div class="environment-row">
          <span>分支</span>
          <span>{props.branch}</span>
        </div>
      </Show>
      <Show when={props.session.environment.changedFiles.length > 0}>
        <div class="environment-files">
          <For each={props.session.environment.changedFiles.slice(0, 8)}>
            {(file) => <code>{file}</code>}
          </For>
        </div>
      </Show>
    </aside>
  );
}
