// @vitest-environment jsdom

import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRawSessionState } from "../../store/sessionEventReducer";
import EnvironmentPopover from "./EnvironmentPopover";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.innerHTML = ""; });

describe("EnvironmentPopover tasks", () => {
  it("renders session tasks and forwards task actions", () => {
    const host = document.createElement("div");
    document.body.append(host);
    const onTaskAction = vi.fn();
    const task = { id: "T1", subject: "实现审批", description: "接通计划审核", status: "in_progress" };
    dispose = render(() => (
      <EnvironmentPopover
        session={createRawSessionState("seed-1")}
        workspace="F:/repo"
        tasks={[task]}
        onTaskAction={onTaskAction}
      />
    ), host);

    expect(host.textContent).toContain("T1");
    expect(host.textContent).toContain("实现审批");
    host.querySelector<HTMLButtonElement>(".environment-task-main")!.click();
    expect(onTaskAction).toHaveBeenCalledWith("ask", task);
    host.querySelector<HTMLButtonElement>(".environment-task-action")!.click();
    expect(onTaskAction).toHaveBeenCalledWith("cancel", task);
  });
});
