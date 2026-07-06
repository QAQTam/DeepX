import { For, Show, createSignal, createResource, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
    return [];
  }
}

export default function PlanReviewPanel(props: { seed: string }) {
  const { t } = useI18n();
  const [planItems, { refetch }] = createResource(
    () => props.seed,
    fetchPlan,
  );

  listen("plan-changed", (e: { payload: { seed: string } }) => {
    if (e.payload.seed === props.seed) refetch();
  }).then(unlisten => onCleanup(unlisten));

  const [selected, setSelected] = createSignal<Set<string>>(new Set<string>());
  const [followUps, setFollowUps] = createSignal<Record<string, string>>({});
  const [busy, setBusy] = createSignal(false);
  const [confirmAction, setConfirmAction] = createSignal<"approved" | "rejected" | null>(null);

  const toggle = (id: string) => {
    const next = new Set(selected());
    if (next.has(id)) next.delete(id); else next.add(id);
    setSelected(next);
  };

  const toggleAll = () => {
    const items = planItems();
    if (!items) return;
    if (selected().size === items.length) {
      setSelected(new Set<string>());
    } else {
      setSelected(new Set<string>(items.map(i => i.id)));
    }
  };

  const setFollowUp = (id: string, text: string) => {
    setFollowUps(prev => ({ ...prev, [id]: text }));
  };

  const doBatchAction = async (action: "approved" | "rejected") => {
    setConfirmAction(action);
  };

  const confirmBatch = async () => {
    const action = confirmAction();
    if (!action) return;
    setBusy(true);
    const ids = [...selected()];
    let ok = 0;
    for (const id of ids) {
      try {
        const comment = followUps()[id] || "";
        await invoke("cmd_plan_action", {
          seed: props.seed,
          itemId: id,
          action,
          userComment: comment,
        });
        ok++;
      } catch (e) {
        console.error(`plan_action ${id}:`, e);
      }
    }
    setBusy(false);
    setConfirmAction(null);
    setSelected(new Set<string>());
    setFollowUps({});
    await refetch();
    console.log(`[plan] batch ${action}: ${ok}/${ids.length} ok`);
  };

  const cancelConfirm = () => setConfirmAction(null);

  const statusLabel = (s: string) => {
    switch (s) {
      case "approved": return t().planReview?.approved ?? "Approved";
      case "rejected": return t().planReview?.rejected ?? "Rejected";
      case "ask": return t().planReview?.ask ?? "Question";
      default: return t().planReview?.pending ?? "Pending";
    }
  };

  const statusClass = (s: string) => `plan-status-${s}`;

  const selectedCount = () => selected().size;
  const totalCount = () => planItems()?.length ?? 0;

  return (
    <div class="plan-review-full">
      {/* Confirm dialog overlay */}
      <Show when={confirmAction()}>
        <div class="plan-confirm-overlay" onClick={cancelConfirm}>
          <div class="plan-confirm-dialog" onClick={(e) => e.stopPropagation()}>
            <div class="plan-confirm-title">
              {confirmAction() === "approved" ? "批准" : "拒绝"} {selectedCount()} 项？
            </div>
            <div class="plan-confirm-body">
              <For each={planItems()?.filter(i => selected().has(i.id))}>
                {(item) => (
                  <div class="plan-confirm-item">
                    <span class={statusClass(item.status)}>{item.id}: {item.title}</span>
                    <Show when={followUps()[item.id]}>
                      <span class="plan-confirm-followup">追问: {followUps()[item.id]}</span>
                    </Show>
                  </div>
                )}
              </For>
            </div>
            <div class="plan-confirm-actions">
              <button class="plan-btn plan-btn-cancel" onClick={cancelConfirm}>取消</button>
              <button class="plan-btn plan-btn-confirm" onClick={confirmBatch} disabled={busy()}>
                {busy() ? "提交中…" : "确认提交"}
              </button>
            </div>
          </div>
        </div>
      </Show>

      {/* Header with select-all */}
      <div class="plan-review-hd">
        <label class="plan-check-all">
          <input type="checkbox" checked={selectedCount() === totalCount() && totalCount() > 0}
            onChange={toggleAll} disabled={confirmAction() !== null} />
          <span>全选 ({selectedCount()}/{totalCount()})</span>
        </label>
      </div>

      {/* Plan items */}
      <Show
        when={planItems()?.length}
        fallback={<div class="status-empty">{t().planReview?.empty ?? "No PLAN.md found in workspace."}</div>}
      >
        <div class="plan-review-list">
          <For each={planItems()}>
            {(item) => (
              <div class={`plan-row ${statusClass(item.status)} ${selected().has(item.id) ? "selected" : ""}`}>
                <label class="plan-row-check">
                  <input type="checkbox" checked={selected().has(item.id)}
                    onChange={() => toggle(item.id)}
                    disabled={confirmAction() !== null} />
                </label>
                <div class="plan-row-body" onClick={() => toggle(item.id)}>
                  <span class="plan-row-icon">
                    {item.status === "approved" ? "✓" :
                     item.status === "rejected" ? "✗" :
                     item.status === "ask" ? "?" : "○"}
                  </span>
                  <div class="plan-row-info">
                    <span class="plan-row-title">{item.id}: {item.title}</span>
                    <span class="plan-row-status">{statusLabel(item.status)}</span>
                    <Show when={item.comment}>
                      <span class="plan-comment">{item.comment}</span>
                    </Show>
                  </div>
                </div>
                <div class="plan-row-followup">
                  <input
                    type="text"
                    class="plan-followup-input"
                    placeholder="追问理由…"
                    value={followUps()[item.id] || ""}
                    onInput={(e) => setFollowUp(item.id, e.currentTarget.value)}
                    onClick={(e) => e.stopPropagation()}
                  />
                </div>
              </div>
            )}
          </For>
        </div>
      </Show>

      {/* Batch action bar */}
      <Show when={selectedCount() > 0 && !confirmAction()}>
        <div class="plan-batch-bar">
          <button class="plan-btn plan-btn-approve" onClick={() => doBatchAction("approved")}>
            ✓ 批准选中 ({selectedCount()})
          </button>
          <button class="plan-btn plan-btn-reject" onClick={() => doBatchAction("rejected")}>
            ✗ 拒绝选中 ({selectedCount()})
          </button>
        </div>
      </Show>
    </div>
  );
}
