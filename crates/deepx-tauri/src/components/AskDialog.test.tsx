// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { expect, it } from "vitest";

import type { AskState } from "../store/chat";
import AskDialog from "./AskDialog";

function single(askId: string, option: string): AskState {
  return {
    askId,
    mode: "single",
    show: true,
    questions: [
      { id: "q1", question: `${askId}?`, options: [option], allow_custom: true },
    ],
  };
}

it("clears custom input when the active ask_id changes", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const [state, setState] = createSignal(single("ask-1", "A"));
  const dispose = render(
    () => <AskDialog state={state} onSubmit={() => {}} onDismiss={() => {}} />,
    host,
  );

  const input = host.querySelector("input") as HTMLInputElement;
  input.value = "stale custom answer";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  expect(input.value).toBe("stale custom answer");

  setState(single("ask-2", "B"));
  expect((host.querySelector("input") as HTMLInputElement).value).toBe("");

  dispose();
  host.remove();
});
