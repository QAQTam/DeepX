// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AskState } from "../store/chat";
import type { QueuedPermission } from "../store/permissionQueue";
import { createI18n, I18nCtx } from "../i18n";
import ChatView from "./ChatView";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
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
        permission={options?.permission}
        onPermissionRespond={options?.onPermissionRespond}
      />
    </I18nCtx.Provider>
  ), host));
  return host;
}

describe("ChatView inline interactions", () => {
  it("renders ask_user in a dock immediately above the composer", async () => {
    const chat = makeChat(() => ({
      askId: "ask-1",
      mode: "single",
      questions: [{ id: "q1", question: "Continue?", options: ["yes"], allow_custom: false }],
      show: true,
    }));
    const host = mountChat(chat);

    const dock = host.querySelector(".interaction-dock");
    expect(dock).not.toBeNull();
    expect(dock?.nextElementSibling).toBe(host.querySelector(".composer-wrap"));
    expect(host.querySelector(".ask-overlay")).toBeNull();

    host.querySelector<HTMLButtonElement>(".interaction-option")!.click();
    host.querySelector<HTMLButtonElement>(".interaction-submit")!.click();
    await flush();
    expect(chat.submitAskAnswer).toHaveBeenCalledWith([
      { question_id: "q1", answer: "yes" },
    ]);
  });

  it("renders high-risk permission in the dock and forwards the response", async () => {
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

    expect(host.querySelector(".interaction-dock")?.nextElementSibling)
      .toBe(host.querySelector(".composer-wrap"));
    const approve = host.querySelector<HTMLButtonElement>(".approval-high");
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
