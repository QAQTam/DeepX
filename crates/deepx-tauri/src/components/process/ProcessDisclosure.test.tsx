// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { describe, expect, it } from "vitest";
import type { TurnViewModel } from "../../presentation/turnProjection";
import ProcessDisclosure, { defaultOpenForStatus } from "./ProcessDisclosure";

describe("ProcessDisclosure", () => {
  it("opens active and failed traces but collapses completed traces", () => {
    expect(defaultOpenForStatus("running")).toBe(true);
    expect(defaultOpenForStatus("waiting")).toBe(true);
    expect(defaultOpenForStatus("failed")).toBe(true);
    expect(defaultOpenForStatus("cancelled")).toBe(true);
    expect(defaultOpenForStatus("completed")).toBe(false);
  });

  it("forces a running trace closed when it completes", async () => {
    const host = document.createElement("div");
    const [status, setStatus] = createSignal<TurnViewModel["process"]["status"]>("running");
    const dispose = render(() => (
      <ProcessDisclosure process={{ status: status(), elapsedMs: 900, items: [] }} />
    ), host);

    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("true");
    setStatus("completed");
    await Promise.resolve();
    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("false");
    dispose();
  });

  it("keeps a completed trace open across projection refreshes", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const [process, setProcess] = createSignal<TurnViewModel["process"]>({
      status: "running", elapsedMs: 900, items: [],
    });
    const dispose = render(() => <ProcessDisclosure process={process()} />, host);

    setProcess({ status: "completed", elapsedMs: 1000, items: [] });
    await new Promise((resolve) => setTimeout(resolve, 0));
    host.querySelector<HTMLButtonElement>("button")!.click();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("true");

    setProcess({ status: "completed", elapsedMs: 1100, items: [] });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(host.querySelector("button")?.getAttribute("aria-expanded")).toBe("true");
    dispose();
    host.remove();
  });
});
