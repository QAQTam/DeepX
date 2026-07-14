// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AskState } from "../store/chat";
import type { QueuedPermission } from "../store/permissionQueue";
import { createI18n, I18nCtx } from "../i18n";
import ChatView from "./ChatView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(vi.fn()) }));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn().mockResolvedValue(undefined) }));

const cleanups: Array<() => void> = [];

afterEach(() => {
  cleanups.splice(0).forEach((dispose) => dispose());
  document.body.innerHTML = "";
});

function flush() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function makeChat(askState: () => AskState) {
  const compactResult = (): number | null => null;
  return {
    sessionInfo: { seed: "seed-1", model: "test-model" },
    isStreaming: () => false,
    clearError: vi.fn(),
    workspace: () => "F:/repo",
    askState,
    submitAskAnswer: vi.fn().mockResolvedValue(undefined),
    dismissAsk: vi.fn().mockResolvedValue(undefined),
    isCompacting: () => false,
    compactText: () => "",
    compactResult,
  };
}

function mountChat(chat: ReturnType<typeof makeChat>, options?: {
  permission?: () => QueuedPermission | null;
  onPermissionRespond?: (
    permission: QueuedPermission,
    approved: boolean,
    trustFolder: boolean,
  ) => Promise<void>;
  planReviewOpen?: () => boolean;
  onPlanReviewClose?: () => void;
}) {
  const host = document.createElement("div");
  document.body.append(host);
  const i18n = createI18n("zh");
  cleanups.push(render(() => (
    <I18nCtx.Provider value={i18n}>
      <ChatView
        chat={chat as never}
        rawSession={() => undefined}
        hasMore={false}
        onLoadMore={vi.fn()}
        onSlashCommand={vi.fn()}
        permissionLevel={2}
        onPermissionLevelChange={vi.fn()}
        onChangeWorkspace={vi.fn()}
        permission={options?.permission}
        onPermissionRespond={options?.onPermissionRespond}
        planReviewOpen={options?.planReviewOpen}
        onPlanReviewClose={options?.onPlanReviewClose}
      />
    </I18nCtx.Provider>
  ), host));
  return host;
}

describe("ChatView blocking interactions", () => {
  it("renders plan review in the centered modal", async () => {
    const chat = makeChat(() => ({ askId: "", mode: "single", questions: [], show: false }));
    mountChat(chat, { planReviewOpen: () => true });
    await flush();

    const dialog = document.body.querySelector('[role="dialog"]');
    expect(dialog?.getAttribute("aria-label")).toBe("审核执行计划");
    expect(dialog?.querySelector(".plan-review-prompt")).not.toBeNull();
  });

  it("renders ask_user in a centered modal outside the chat layout", async () => {
    const chat = makeChat(() => ({
      askId: "ask-1",
      mode: "single",
      questions: [{ id: "q1", question: "Continue?", options: ["yes"], allow_custom: false }],
      show: true,
    }));
    const host = mountChat(chat);

    const dialog = document.body.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();
    expect(host.querySelector(".ask-user-prompt")).toBeNull();
    expect(dialog?.querySelector(".ask-user-prompt")).not.toBeNull();

    dialog!.querySelector<HTMLButtonElement>(".interaction-option")!.click();
    dialog!.querySelector<HTMLButtonElement>(".interaction-submit")!.click();
    await flush();
    expect(chat.submitAskAnswer).toHaveBeenCalledWith([
      { question_id: "q1", answer: "yes" },
    ]);
  });

  it("renders high-risk permission in the centered modal and forwards the response", async () => {
    const chat = makeChat(() => ({ askId: "", mode: "single", questions: [], show: false }));
    const permission: QueuedPermission = {
      seed: "seed-1",
      request: {
        tool_call_id: "call-1",
        tool_name: "exec_run",
        reason: "Run command",
        paths: ["F:/repo"],
        category: "exec",
        level: 1,
        risk: "high",
        consequence: "May execute arbitrary commands.",
      },
    };
    const onPermissionRespond = vi.fn().mockResolvedValue(undefined);
    const host = mountChat(chat, {
      permission: () => permission,
      onPermissionRespond,
    });

    const dialog = document.body.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();
    expect(host.querySelector(".permission-prompt")).toBeNull();
    const approve = dialog!.querySelector<HTMLButtonElement>(".approval-high");
    expect(approve).not.toBeNull();
    approve!.click();
    await flush();
    expect(onPermissionRespond).toHaveBeenCalledWith(permission, true, false);
  });

  it("renders active and completed compaction status from chat signals", async () => {
    const [active, setActive] = createSignal(true);
    const [result, setResult] = createSignal<number | null>(null);
    const chat = {
      ...makeChat(() => ({ askId: "", mode: "single", questions: [], show: false })),
      isCompacting: active,
      compactText: () => "",
      compactResult: result,
    };
    const host = mountChat(chat);

    expect(host.querySelector(".compact-active")?.textContent).toContain("正在整理上下文");
    setActive(false);
    setResult(8);
    await flush();
    expect(host.querySelector(".compact-complete")?.textContent).toContain("8 轮对话");
  });
});
