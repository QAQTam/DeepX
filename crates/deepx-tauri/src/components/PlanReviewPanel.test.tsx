// @vitest-environment jsdom

import { render } from "@solidjs/web";
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
    dispose = render(() => (
      <PlanReviewPanel
        planContent="## PLAN\n- [ ] P1: test item"
        onApprove={onApprove}
        onReject={onReject}
      />
    ), host);
    await flush();

    expect(host.textContent).toContain("P1");
    expect(host.querySelector(".interaction-approve")).not.toBeNull();
    expect(host.querySelector(".interaction-reject")).not.toBeNull();
  });

  it("delegates approval exactly once without invoking Tauri itself", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const onApprove = vi.fn().mockResolvedValue(undefined);
    const onReject = vi.fn();
    dispose = render(() => (
      <PlanReviewPanel
        planContent="test plan"
        onApprove={onApprove}
        onReject={onReject}
      />
    ), host);
    await flush();

    host.querySelector<HTMLButtonElement>(".interaction-approve")!.click();
    await flush();

    expect(onApprove).toHaveBeenCalledWith(false);
    expect(invoke).not.toHaveBeenCalled();
    expect(host.querySelector(".plan-review-close")).toBeNull();
  });

  it("passes the explicit goal-mode opt-in with approval", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const onApprove = vi.fn().mockResolvedValue(undefined);
    dispose = render(() => (
      <PlanReviewPanel planContent="test plan" onApprove={onApprove} onReject={vi.fn()} />
    ), host);
    await flush();

    host.querySelector<HTMLInputElement>(".plan-goal-mode input")!.click();
    host.querySelector<HTMLButtonElement>(".interaction-approve")!.click();
    await flush();

    expect(onApprove).toHaveBeenCalledWith(true);
  });
});
