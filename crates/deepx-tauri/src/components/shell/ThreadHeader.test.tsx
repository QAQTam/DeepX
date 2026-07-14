// @vitest-environment jsdom

import { render } from "solid-js/web";
import { describe, expect, it, vi } from "vitest";
import ThreadHeader from "./ThreadHeader";

describe("ThreadHeader", () => {
  it("shows explicit workspace and compaction actions", () => {
    const host = document.createElement("div");
    document.body.append(host);
    const changeWorkspace = vi.fn();
    const compact = vi.fn();
    const dispose = render(() => (
      <ThreadHeader
        title="Task"
        workspace="F:/DeepX-Fork"
        compacting={false}
        environmentOpen={false}
        onToggleEnvironment={vi.fn()}
        onOpenLocation={vi.fn()}
        onChangeWorkspace={changeWorkspace}
        onCompact={compact}
      />
    ), host);

    expect(host.textContent).toContain("DeepX-Fork");
    expect(host.textContent).toContain("整理上下文");
    host.querySelector<HTMLButtonElement>("[data-change-workspace]")!.click();
    expect(changeWorkspace).toHaveBeenCalledOnce();
    dispose();
    host.remove();
  });
});
