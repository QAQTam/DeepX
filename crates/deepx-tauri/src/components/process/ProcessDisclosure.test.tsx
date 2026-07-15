// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { describe, expect, it } from "vitest";
import ProcessDisclosure from "./ProcessDisclosure";

describe("ProcessDisclosure", () => {
  it("defaults open for running, closed for completed", () => {
    const host = document.createElement("div");
    const r = render(() => <ProcessDisclosure status="running" />, host);
    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("true");
    r();

    const host2 = document.createElement("div");
    const r2 = render(() => <ProcessDisclosure status="completed" />, host2);
    expect(host2.querySelector("button")?.getAttribute("aria-expanded")).toBe("false");
    r2();
  });

  it("auto-closes on completion when defaultOpen is not set", async () => {
    const host = document.createElement("div");
    const [status, setStatus] = createSignal<"running" | "completed">("running");
    const dispose = render(() => (
      <ProcessDisclosure status={status()} />
    ), host);

    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("true");
    setStatus("completed");
    await Promise.resolve();
    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("false");
    dispose();
  });

  it("respects explicit defaultOpen and ignores auto-close", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const dispose = render(() => (
      <ProcessDisclosure status="completed" defaultOpen={true} />
    ), host);

    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("true");
    // Should stay open even though status is completed
    await Promise.resolve();
    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("true");
    dispose();
    host.remove();
  });
});
