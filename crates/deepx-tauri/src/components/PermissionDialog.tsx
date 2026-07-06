import { createSignal, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

export interface PermissionRequest {
  tool_call_id: string;
  tool_name: string;
  reason: string;
  paths: string[];
  category: string;
  level: number;
}

interface Props {
  request: PermissionRequest;
  seed: string;
  onClose: () => void;
}

const CATEGORY_LABELS: Record<string, string> = {
  read: "📖 Read",
  write: "✏️ Write",
  exec: "⚡ Execute",
  net: "🌐 Network",
};

const CATEGORY_COLORS: Record<string, string> = {
  read: "#5a8a4a",
  write: "#d4783c",
  exec: "#c4553d",
  net: "#6b8db5",
};

export default function PermissionDialog(props: Props) {
  const [trustFolder, setTrustFolder] = createSignal(false);
  const [busy, setBusy] = createSignal(false);

  async function respond(approved: boolean) {
    setBusy(true);
    try {
      await invoke("cmd_permission_response", {
        seed: props.seed,
        toolCallId: props.request.tool_call_id,
        approved,
        trustFolder: approved && trustFolder(),
      });
    } catch (e) {
      console.error(`permission_response:`, e);
    } finally {
      setBusy(false);
      props.onClose();
    }
  }

  const isCrossWorkspace = props.request.reason.includes("outside the workspace") ||
    props.request.reason.includes("Level 3");

  return (
    <div class="perm-overlay" onClick={() => respond(false)}>
      <div class="perm-dialog" onClick={(e) => e.stopPropagation()}>
        <div class="perm-header">
          <span class="perm-category" style={{ color: CATEGORY_COLORS[props.request.category] ?? "#888" }}>
            {CATEGORY_LABELS[props.request.category] ?? props.request.category}
          </span>
          <span class="perm-level-badge">Level {props.request.level}</span>
        </div>

        <div class="perm-tool-name">{props.request.tool_name}</div>

        <div class="perm-reason">{props.request.reason}</div>

        <Show when={props.request.paths.length > 0}>
          <div class="perm-paths">
            <div class="perm-paths-label">Target paths:</div>
            {props.request.paths.map(p => (
              <code class="perm-path">{p}</code>
            ))}
          </div>
        </Show>

        <Show when={isCrossWorkspace}>
          <label class="perm-checkbox">
            <input type="checkbox" checked={trustFolder()} onChange={(e) => setTrustFolder(e.currentTarget.checked)} />
            <span>Trust this folder permanently (skip future prompts)</span>
          </label>
        </Show>

        <div class="perm-actions">
          <button class="perm-btn perm-btn-deny" onClick={() => respond(false)} disabled={busy()}>
            ✗ Deny
          </button>
          <button class="perm-btn perm-btn-allow" onClick={() => respond(true)} disabled={busy()}>
            ✓ Allow
          </button>
        </div>
      </div>
    </div>
  );
}
