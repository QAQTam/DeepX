import { createSignal, For, Show } from "solid-js";
import type { PermissionRisk } from "../../lib/types";
import type { PermissionRequest } from "../../store/permissionQueue";

export const approvalClass = (risk: PermissionRisk): string =>
  ({
    low: "approval-low",
    medium: "approval-medium",
    high: "approval-high",
  }[risk]);

export const approvalLabel = (risk: PermissionRisk, category: string): string => {
  if (risk === "high") {
    if (category === "exec") return "批准并执行";
    return "批准并继续";
  }
  return "批准";
};

export default function PermissionPrompt(props: {
  request: PermissionRequest;
  onRespond: (approved: boolean, trustFolder: boolean) => void | Promise<void>;
}) {
  const [busy, setBusy] = createSignal(false);
  const [trustFolder, setTrustFolder] = createSignal(false);

  const respond = async (approved: boolean) => {
    if (busy()) return;
    setBusy(true);
    try {
      await props.onRespond(approved, approved && trustFolder());
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      class="interaction-prompt permission-prompt"
      aria-labelledby="permission-heading"
    >
      <div class="interaction-eyebrow">需要授权</div>
      <h3 id="permission-heading">{props.request.tool_name}</h3>
      <div class="permission-meta">
        <span data-permission-category>{props.request.category}</span>
        <span data-permission-risk>{props.request.risk}</span>
      </div>
      <p class="interaction-reason">{props.request.reason}</p>
      <p class="interaction-consequence">{props.request.consequence}</p>
      <Show when={props.request.paths.length > 0}>
        <div class="interaction-paths">
          <For each={props.request.paths}>{(path) => <code>{path}</code>}</For>
        </div>
      </Show>
      <Show when={props.request.risk === "high" && props.request.paths.length > 0}>
        <label class="interaction-trust">
          <input
            type="checkbox"
            checked={trustFolder()}
            onChange={(event) => setTrustFolder(event.currentTarget.checked)}
            disabled={busy()}
          />
          信任此目录
        </label>
      </Show>
      <div class="interaction-actions">
        <button
          type="button"
          class="interaction-reject"
          data-reject
          disabled={busy()}
          onClick={() => respond(false)}
        >
          拒绝
        </button>
        <button
          type="button"
          class={`interaction-approve ${approvalClass(props.request.risk)}`}
          data-approve
          disabled={busy()}
          onClick={() => respond(true)}
        >
          {approvalLabel(props.request.risk, props.request.category)}
        </button>
      </div>
    </section>
  );
}
