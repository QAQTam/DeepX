// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { expect, it } from "vitest";

import type { AskAnswer } from "../lib/types";
import type { AskState } from "../store/chat";
import AskForm from "./AskForm";

function batch(askId: string, options: [string, string]): AskState {
  return {
    askId,
    mode: "batch",
    show: true,
    questions: [
      { id: "q1", question: "First?", options: [options[0]], allow_custom: true },
      { id: "q2", question: "Second?", options: [options[1]], allow_custom: false },
    ],
  };
}

it("clears local answers when the active ask_id changes", () => {
  const host = document.createElement("div");
  document.body.append(host);
  const [state, setState] = createSignal(batch("ask-1", ["A1", "A2"]));
  const submissions: AskAnswer[][] = [];
  const dispose = render(
    () => (
      <AskForm
        state={state}
        onSubmit={(answers) => submissions.push(answers)}
        onDismiss={() => {}}
      />
    ),
    host,
  );

  const click = (label: string) => {
    const button = [...host.querySelectorAll("button")].find(
      (item) => item.textContent?.trim() === label,
    );
    expect(button, `button ${label}`).toBeDefined();
    button!.click();
  };

  // Page 0: click A1
  click("A1");
  // Navigate to page 1
  click("Next");
  click("A2");

  // Switch ask — should reset page to 0 and clear answers
  setState(batch("ask-2", ["B1", "B2"]));
  click("Submit");
  expect(submissions).toHaveLength(0);

  // Page 0: type custom then click B1
  const custom = host.querySelector("input") as HTMLInputElement;
  custom.value = "custom that should be replaced";
  custom.dispatchEvent(new Event("input", { bubbles: true }));
  click("B1");
  // Navigate to page 1 and click B2
  click("Next");
  click("B2");
  click("Submit");

  expect(submissions).toEqual([
    [
      { question_id: "q1", answer: "B1" },
      { question_id: "q2", answer: "B2" },
    ],
  ]);

  dispose();
  host.remove();
});
