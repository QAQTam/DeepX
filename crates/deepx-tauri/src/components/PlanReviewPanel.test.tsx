// @vitest-environment jsdom

import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import PlanReviewPanel from "./PlanReviewPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(vi.fn()) }));

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.innerHTML = ""; vi.clearAllMocks(); });

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("PlanReviewPanel", () => {
  it("shows plan content and renders approve/reject buttons", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const onApprove = vi.fn();
    const onReject = vi.fn();
    const onClose = vi.fn();
    dispose = render(() => (
      <PlanReviewPanel
        seed="seed-1"
        callId="call-1"
        planContent="## PLAN\n- [ ] P1: test item"
        onApprove={onApprove}
        onReject={onReject}
        onClose={onClose}
      />
    ), host);
    await flush();

    expect(host.textContent).toContain("P1");
    expect(host.querySelector(".interaction-approve")).not.toBeNull();
    expect(host.querySelector(".interaction-reject")).not.toBeNull();
  });

  it("calls onApprove and cmd_plan_review when approved", async () => {
    vi.mocked(invoke).mockResolvedValue(undefined as never);
    const host = document.createElement("div");
    document.body.append(host);
    const onApprove = vi.fn();
    const onReject = vi.fn();
    const onClose = vi.fn();
    dispose = render(() => (
      <PlanReviewPanel
        seed="seed-1"
        callId="call-1"
        planContent="test plan"
        onApprove={onApprove}
        onReject={onReject}
        onClose={onClose}
      />
    ), host);
    await flush();

    host.querySelector<HTMLButtonElement>(".interaction-approve")!.click();
    await flush();

    expect(invoke).toHaveBeenCalledWith("cmd_plan_review", expect.objectContaining({
      seed: "seed-1",
      callId: "call-1",
      approved: true,
    }));
    expect(onApprove).toHaveBeenCalledOnce();
  });
});
