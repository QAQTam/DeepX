// @vitest-environment jsdom

import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { expect, it, vi } from "vitest";

const shikiState = vi.hoisted(() => {
  let resolve!: (value: { codeToHtml: (text: string) => string }) => void;
  const promise = new Promise<{ codeToHtml: (text: string) => string }>(r => {
    resolve = r;
  });
  return { promise, resolve };
});

vi.mock("shiki", () => ({
  createHighlighter: vi.fn(() => shikiState.promise),
  createOnigurumaEngine: vi.fn(() => ({})),
}));

import MarkdownBody from "./MarkdownBody";

it("shows final text immediately and ignores an older async render", async () => {
  const host = document.createElement("div");
  const [content, setContent] = createSignal("old answer");
  const dispose = render(
    () => <MarkdownBody content={content()} final={true} />,
    host,
  );

  expect(host.textContent).toContain("old answer");
  setContent("new answer");
  expect(host.textContent).toContain("new answer");

  shikiState.resolve({
    codeToHtml: text => `<pre><code>${text}</code></pre>`,
  });

  await vi.waitFor(() => expect(host.textContent).toContain("new answer"));
  expect(host.textContent).not.toContain("old answer");
  dispose();
});
