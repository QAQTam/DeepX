export default function PlanApprovalPrompt(props: {
  summary: string;
  onAction: (action: "approve" | "revise" | "reject") => void | Promise<void>;
}) {
  return (
    <section class="interaction-prompt plan-approval-prompt">
      <div class="interaction-eyebrow">计划审核</div>
      <p>{props.summary}</p>
      <div class="interaction-actions">
        <button type="button" class="interaction-reject" onClick={() => props.onAction("reject")}>拒绝</button>
        <button type="button" class="interaction-secondary" onClick={() => props.onAction("revise")}>要求修改</button>
        <button type="button" class="interaction-approve approval-low" onClick={() => props.onAction("approve")}>批准计划</button>
      </div>
    </section>
  );
}
