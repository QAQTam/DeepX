// @vitest-environment jsdom

import { render } from "@solidjs/web";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createI18n, I18nCtx } from "../i18n";
import { request } from "../runtime/backendClient";
import GoalStatusStrip from "./GoalStatusStrip";

vi.mock("../runtime/backendClient", () => ({ request: vi.fn() }));

const requestMock = vi.mocked(request);
const cleanups: Array<() => void> = [];
const flush = () => new Promise(resolve => setTimeout(resolve, 0));

afterEach(() => {
  cleanups.splice(0).forEach(dispose => dispose());
  document.body.innerHTML = "";
  vi.resetAllMocks();
});

function mount() {
  const [refreshKey, setRefreshKey] = createSignal("1:turn:running");
  const host = document.createElement("div");
  document.body.append(host);
  cleanups.push(render(() => (
    <I18nCtx value={createI18n("zh")}>
      <GoalStatusStrip seed="goal-seed" refreshKey={refreshKey()} />
    </I18nCtx>
  ), host));
  return setRefreshKey;
}

describe("GoalStatusStrip", () => {
  it("retracts after the finishing turn refreshes a completed goal", async () => {
    requestMock
      .mockResolvedValueOnce({
        objective: "完成迁移",
        status: "active",
        current_id: "step-1",
        current_title: "实现",
        completed: 0,
        total: 1,
      })
      .mockResolvedValueOnce({
        objective: "完成迁移",
        status: "completed",
        completed: 1,
        total: 1,
      });
    const setRefreshKey = mount();

    await flush();
    expect(document.querySelector(".goal-status-strip")?.textContent).toContain("完成迁移");

    setRefreshKey("1:turn:completed");
    await flush();
    expect(document.querySelector(".goal-status-strip")).toBeNull();
    expect(requestMock).toHaveBeenCalledTimes(2);
  });

  it("does not render a terminal goal restored from disk", async () => {
    requestMock.mockResolvedValueOnce({
      objective: "旧目标",
      status: "stopped",
      completed: 0,
      total: 2,
    });
    mount();

    await flush();
    expect(document.querySelector(".goal-status-strip")).toBeNull();
  });

  it("ignores an older active response that arrives after completion", async () => {
    let resolveActive!: (value: unknown) => void;
    let resolveCompleted!: (value: unknown) => void;
    requestMock
      .mockImplementationOnce(() => new Promise(resolve => { resolveActive = resolve; }))
      .mockImplementationOnce(() => new Promise(resolve => { resolveCompleted = resolve; }));
    const setRefreshKey = mount();
    await flush();
    setRefreshKey("1:turn:completed");
    await flush();

    resolveCompleted({ objective: "完成迁移", status: "completed", completed: 1, total: 1 });
    await flush();
    resolveActive({ objective: "完成迁移", status: "active", completed: 0, total: 1 });
    await flush();

    expect(document.querySelector(".goal-status-strip")).toBeNull();
  });
});
