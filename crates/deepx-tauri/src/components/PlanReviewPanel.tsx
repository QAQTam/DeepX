import { Show, createSignal } from "solid-js";

interface PlanReviewPanelProps {
  planContent: string;
  onApprove: (autonomous: boolean) => void | Promise<void>;
  onReject: (message?: string) => void | Promise<void>;
}

export default function PlanReviewPanel(props: PlanReviewPanelProps) {
  const [feedback, setFeedback] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [autonomous, setAutonomous] = createSignal(false);

  async function handleApprove(autonomousOverride = autonomous()) {
    if (busy()) return;
    setBusy(true);
    try {
      await props.onApprove(autonomousOverride);
    } finally {
      setBusy(false);
    }
  }

  async function handleReject() {
    if (busy()) return;
    const message = feedback().trim() || undefined;
    setBusy(true);
    try {
      await props.onReject(message);
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
      <label class="plan-goal-mode">
        <input
          type="checkbox"
          checked={autonomous()}
          onChange={(event) => setAutonomous(event.currentTarget.checked)}
        />
        <span>
          <strong>以目标模式执行</strong>
          <small>逐项自动推进；每一步完成后会生成新的执行回合，可随时停止。</small>
        </span>
      </label>
      <footer class="plan-review-actions">
        <button type="button" class="interaction-reject" disabled={busy()} onClick={handleReject}>
          拒绝计划
        </button>
        <button type="button" class="interaction-approve" disabled={busy()} onClick={(event) => {
          const enabled = event.currentTarget
            .closest(".plan-review-prompt")
            ?.querySelector<HTMLInputElement>(".plan-goal-mode input")
            ?.checked ?? autonomous();
          void handleApprove(enabled);
        }}>
          {busy() ? "提交中…" : autonomous() ? "批准并启动目标模式" : "批准并继续"}
        </button>
      </footer>
    </section>
  );
}
