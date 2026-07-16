// @vitest-environment jsdom
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { expect, it, vi } from "vitest";
import type { TurnViewModel } from "../../presentation/turnProjection";
import ConversationTranscript from "./ConversationTranscript";

it("loads older turns from a real transcript control", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const onLoadMore = vi.fn();
  const dispose = render(() => (
    <ConversationTranscript turns={[]} hasMore={true} onLoadMore={onLoadMore} />
  ), host);
  host.querySelector<HTMLButtonElement>("[data-load-more]")!.click();
  expect(onLoadMore).toHaveBeenCalledOnce();
  dispose();
});

it("preserves viewport distance when older turns prepend", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const [turns, setTurns] = createSignal<TurnViewModel[]>([{
    turnId: "new", userPrompt: "new", status: "completed", rounds: [], interactions: [],
  }]);
  let height = 1000;
  const dispose = render(() => <ConversationTranscript
    turns={turns()} hasMore={true}
    onLoadMore={() => {
      height = 1200;
      setTurns(current => [{
        turnId: "old", userPrompt: "old", status: "completed", rounds: [], interactions: [],
      }, ...current]);
    }}
  />, host);
  const scroller = host.querySelector<HTMLElement>(".conversation-scroll")!;
  Object.defineProperty(scroller, "scrollHeight", { get: () => height });
  scroller.scrollTop = 400;
  host.querySelector<HTMLButtonElement>("[data-load-more]")!.click();
  await Promise.resolve();
  await Promise.resolve();
  expect(scroller.scrollTop).toBe(600);
  dispose();
});
