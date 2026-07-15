import { Show, createSignal } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

interface PlanReviewPanelProps {
  seed: string;
  callId: string;
  planContent: string;
  onApprove: () => void;
  onReject: (message?: string) => void;
  onClose: () => void;
}

export default function PlanReviewPanel(props: PlanReviewPanelProps) {
  const [feedback, setFeedback] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  async function handleApprove() {
    if (busy()) return;
    setBusy(true);
    try {
      await invoke("cmd_plan_review", {
        seed: props.seed,
        callId: props.callId,
        approved: true,
        message: null,
      });
      props.onApprove();
    } catch (error) {
      console.error("plan review approve:", error);
    } finally {
      setBusy(false);
    }
  }

  async function handleReject() {
    if (busy()) return;
    const message = feedback().trim() || undefined;
    setBusy(true);
    try {
      await invoke("cmd_plan_review", {
        seed: props.seed,
        callId: props.callId,
        approved: false,
        message: message ?? null,
      });
      props.onReject(message);
    } catch (error) {
      console.error("plan review reject:", error);
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
          <p>审阅计划内容后批准执行，或留下拒绝原因。</p>
        </div>
        <button type="button" class="plan-review-close" aria-label="稍后审核" onClick={props.onClose}>×</button>
      </header>

      <Show when={props.planContent} fallback={<div class="plan-review-empty">计划内容为空。</div>}>
        <pre class="plan-review-content">{props.planContent}</pre>
      </Show>

      <textarea
        class="plan-review-feedback"
        rows={3}
        value={feedback()}
        onInput={(event) => setFeedback(event.currentTarget.value)}
        placeholder="拒绝原因或修改意见（拒绝时可选）"
      />
      <footer class="plan-review-actions">
        <button type="button" class="interaction-reject" disabled={busy()} onClick={handleReject}>
          拒绝计划
        </button>
        <button type="button" class="interaction-approve" disabled={busy()} onClick={handleApprove}>
          {busy() ? "提交中…" : "批准并继续"}
        </button>
      </footer>
    </section>
  );
}
