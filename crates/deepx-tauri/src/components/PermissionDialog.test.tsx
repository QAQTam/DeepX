// @vitest-environment jsdom

import { invoke } from "@tauri-apps/api/core";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { expect, it, vi } from "vitest";

import { createI18n, I18nCtx } from "../i18n";
import PermissionDialog from "./PermissionDialog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

it("keeps the permission active after delivery failure and resolves after retry", async () => {
  const invokeMock = vi.mocked(invoke);
  invokeMock.mockRejectedValueOnce(new Error("offline")).mockResolvedValueOnce(undefined);
  const onResolved = vi.fn();
  const host = document.createElement("div");
  document.body.append(host);
  const i18n = createI18n("en");
  const dispose = render(
    () => (
      <I18nCtx.Provider value={i18n}>
        <PermissionDialog
          seed="listener-seed"
          request={{
            tool_call_id: "call-1",
            tool_name: "shell_command",
            reason: "test",
            paths: [],
            category: "exec",
            level: 2,
          }}
          onResolved={onResolved}
        />
      </I18nCtx.Provider>
    ),
    host,
  );

  const allow = [...host.querySelectorAll("button")].find((button) =>
    button.classList.contains("perm-btn-allow")
  )!;
  allow.click();
  await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
  expect(onResolved).not.toHaveBeenCalled();

  allow.click();
  await vi.waitFor(() => expect(onResolved).toHaveBeenCalledTimes(1));
  expect(invokeMock).toHaveBeenLastCalledWith("cmd_permission_response", expect.objectContaining({
    seed: "listener-seed",
    toolCallId: "call-1",
  }));

  dispose();
  host.remove();
});

it("acknowledges the exact permission captured before invoke", async () => {
  let finishInvoke!: () => void;
  vi.mocked(invoke).mockImplementationOnce(
    () => new Promise<void>((resolve) => { finishInvoke = resolve; }),
  );
  const onResolved = vi.fn();
  const [item, setItem] = createSignal({ seed: "seed-a", id: "call-a" });
  const host = document.createElement("div");
  document.body.append(host);
  const i18n = createI18n("en");
  const dispose = render(
    () => (
      <I18nCtx.Provider value={i18n}>
        <PermissionDialog
          seed={item().seed}
          request={{
            tool_call_id: item().id,
            tool_name: "shell_command",
            reason: "test",
            paths: [],
            category: "exec",
            level: 2,
          }}
          onResolved={onResolved}
        />
      </I18nCtx.Provider>
    ),
    host,
  );

  (host.querySelector(".perm-btn-allow") as HTMLButtonElement).click();
  setItem({ seed: "seed-b", id: "call-b" });
  finishInvoke();

  await vi.waitFor(() => expect(onResolved).toHaveBeenCalledTimes(1));
  expect(onResolved).toHaveBeenCalledWith("seed-a", "call-a");
  dispose();
  host.remove();
});
