import { For, Show, createSignal, createResource } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n";

interface PlanItem {
  id: string;
  title: string;
  status: string;
  comment: string;
  actions: string[];
}

async function fetchPlan(seed: string): Promise<PlanItem[]> {
  try {
    const raw = await invoke<string>("cmd_read_plan", { seed });
    if (!raw || raw === "[]") return [];
    return JSON.parse(raw);
  } catch {
    return [];  // PLAN.md missing or no workspace — silent
  }
}

export default function PlanReviewPanel(props: { seed: string }) {
  const { t } = useI18n();
  const [planItems, { refetch }] = createResource(
    () => props.seed,
    fetchPlan,
  );

  const [busy, setBusy] = createSignal<string | null>(null);

  const doAction = async (itemId: string, action: string) => {
    setBusy(itemId);
    try {
      await invoke("cmd_plan_action", {
        seed: props.seed,
        itemId,
        action,
        userComment: "",
      });
      await refetch();
    } catch (e) {
      console.error("plan action failed:", e);
    } finally {
      setBusy(null);
    }
  };

  const statusLabel = (s: string) => {
    switch (s) {
      case "approved": return t().planReview?.approved ?? "Approved";
      case "rejected": return t().planReview?.rejected ?? "Rejected";
      case "ask": return t().planReview?.ask ?? "Question";
      default: return t().planReview?.pending ?? "Pending";
    }
  };

  const statusClass = (s: string) => `plan-status-${s}`;

  return (
    <div class="status-section">
      <div class="status-section-hd">
        {t().planReview?.title ?? "PLAN Review"}
        <Show when={planItems()?.length}>
          <span class="status-section-badge">
            {planItems()!.filter((i) => i.status !== "pending").length}/{planItems()!.length}
          </span>
        </Show>
      </div>
      <Show
        when={planItems()?.length}
        fallback={<div class="status-empty">{t().planReview?.empty ?? "No PLAN.md found in workspace."}</div>}
      >
        <div class="status-section-body">
          <For each={planItems()}>
            {(item) => (
              <div class={`status-row ${statusClass(item.status)}`}>
                <span class="status-row-icon">
                  {busy() === item.id ? "…" :
                   item.status === "approved" ? "✓" :
                   item.status === "rejected" ? "✗" :
                   item.status === "ask" ? "?" : "○"}
                </span>
                <div class="status-row-info">
                  <span class="status-row-title">{item.id}: {item.title}</span>
                  <span class="status-row-desc">{statusLabel(item.status)}</span>
                  <Show when={item.comment}>
                    <span class="plan-comment">{item.comment}</span>
                  </Show>
                </div>
                <div class="status-row-actions">
                  <button
                    class="task-btn task-btn-approve"
                    disabled={busy() !== null}
                    onClick={() => doAction(item.id, "approved")}
                    title={t().planReview?.approve ?? "Approve"}
                  >✓</button>
                  <button
                    class="task-btn task-btn-reject"
                    disabled={busy() !== null}
                    onClick={() => doAction(item.id, "rejected")}
                    title={t().planReview?.reject ?? "Reject"}
                  >✗</button>
                  <button
                    class="task-btn task-btn-ask"
                    disabled={busy() !== null}
                    onClick={() => doAction(item.id, "ask")}
                    title={t().planReview?.askItem ?? "Ask"}
                  >?</button>
                </div>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}
