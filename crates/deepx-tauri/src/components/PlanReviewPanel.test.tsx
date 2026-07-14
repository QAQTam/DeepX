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
  it("approves every item then continues the agent", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "cmd_read_plan") return JSON.stringify([
        { id: "P1", title: "检查接线", status: "pending", comment: "", actions: [] },
        { id: "P2", title: "完成实现", status: "approved", comment: "", actions: [] },
      ]) as never;
      return undefined as never;
    });
    const host = document.createElement("div");
    document.body.append(host);
    const onClose = vi.fn();
    dispose = render(() => <PlanReviewPanel seed="seed-1" onClose={onClose} />, host);
    await flush();

    host.querySelector<HTMLButtonElement>(".plan-review-actions .interaction-approve")!.click();
    await flush();
    await flush();

    expect(invoke).toHaveBeenCalledWith("cmd_plan_action", expect.objectContaining({ itemId: "P1", action: "approve" }));
    expect(invoke).toHaveBeenCalledWith("cmd_send_message", expect.objectContaining({ seed: "seed-1" }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
