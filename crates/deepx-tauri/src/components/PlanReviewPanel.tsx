import { For, Show, createResource, createSignal, onCleanup } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface PlanItem {
  id: string;
  title: string;
  status: "pending" | "approved" | "rejected" | "ask";
  comment: string;
  actions: string[];
}

async function fetchPlan(seed: string): Promise<PlanItem[]> {
  try {
    const raw = await invoke<string>("cmd_read_plan", { seed });
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

export default function PlanReviewPanel(props: { seed: string; onClose: () => void }) {
  const [planItems, { refetch }] = createResource(() => props.seed, fetchPlan);
  const [feedback, setFeedback] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  listen("plan-changed", (event: { payload: { seed: string } }) => {
    if (event.payload.seed === props.seed) void refetch();
  }).then((unlisten) => onCleanup(unlisten));

  async function updateItem(item: PlanItem, action: "approve" | "reject" | "ask") {
    await invoke("cmd_plan_action", {
      seed: props.seed,
      itemId: item.id,
      action,
      userComment: feedback(),
    });
    await refetch();
  }

  async function finish(action: "approve" | "revise" | "reject") {
    if (busy() || (action === "revise" && !feedback().trim())) return;
    setBusy(true);
    try {
      if (action === "approve") {
        for (const item of planItems() ?? []) {
          if (item.status !== "approved") await updateItem(item, "approve");
        }
      }
      const text = action === "approve"
        ? "计划已由用户批准。请严格按已批准的 PLAN.md 继续执行。"
        : action === "revise"
          ? `计划暂未批准。请根据以下审阅意见修改 PLAN.md，并重新提交审核：\n${feedback().trim()}`
          : `用户拒绝了当前计划。请停止执行并等待进一步指示。${feedback().trim() ? `\n原因：${feedback().trim()}` : ""}`;
      await invoke("cmd_send_message", { seed: props.seed, text });
      props.onClose();
    } catch (error) {
      console.error("plan review:", error);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section class="plan-review-prompt">
      <header class="plan-review-header">
        <div>
          <div class="interaction-eyebrow">计划审核</div>
          <h2>确认执行计划</h2>
          <p>逐项审阅后批准执行，或留下修改意见。</p>
        </div>
        <button type="button" class="plan-review-close" aria-label="稍后审核" onClick={props.onClose}>×</button>
      </header>

      <Show when={!planItems.loading} fallback={<div class="plan-review-empty">正在加载计划…</div>}>
        <Show when={(planItems()?.length ?? 0) > 0} fallback={<div class="plan-review-empty">当前工作区没有可审核的计划。</div>}>
          <div class="plan-review-list">
            <For each={planItems()}>
              {(item) => (
                <article class={`plan-review-item status-${item.status}`}>
                  <span class="plan-review-state">
                    {item.status === "approved" ? "✓" : item.status === "rejected" ? "×" : item.status === "ask" ? "?" : "○"}
                  </span>
                  <div class="plan-review-copy">
                    <strong>{item.id} · {item.title}</strong>
                    <Show when={item.comment}><small>{item.comment}</small></Show>
                  </div>
                  <div class="plan-review-item-actions">
                    <button type="button" title="需要修改" onClick={() => void updateItem(item, "ask")}>?</button>
                    <button type="button" title="拒绝此项" onClick={() => void updateItem(item, "reject")}>×</button>
                    <button type="button" class="item-approve" title="批准此项" onClick={() => void updateItem(item, "approve")}>✓</button>
                  </div>
                </article>
              )}
            </For>
          </div>
        </Show>
      </Show>

      <textarea
        class="plan-review-feedback"
        rows={3}
        value={feedback()}
        onInput={(event) => setFeedback(event.currentTarget.value)}
        placeholder="修改意见或拒绝原因（要求修改时必填）"
      />
      <footer class="plan-review-actions">
        <button type="button" class="interaction-reject" disabled={busy()} onClick={() => void finish("reject")}>拒绝计划</button>
        <button type="button" class="interaction-reject" disabled={busy() || !feedback().trim()} onClick={() => void finish("revise")}>要求修改</button>
        <button type="button" class="interaction-approve approval-low" disabled={busy() || planItems.loading || !(planItems()?.length)} onClick={() => void finish("approve")}>{busy() ? "提交中…" : "批准并继续"}</button>
      </footer>
    </section>
  );
}
