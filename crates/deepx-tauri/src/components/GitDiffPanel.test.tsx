// @vitest-environment jsdom

import { invoke } from "@tauri-apps/api/core";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { createI18n, I18nCtx } from "../i18n";
import GitDiffPanel, { type GitFileEntry } from "./GitDiffPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ confirm: vi.fn(), open: vi.fn() }));

const invokeMock = vi.mocked(invoke);

function file(overrides: Partial<GitFileEntry> = {}): GitFileEntry {
  return {
    path: "src/main.rs",
    change: "modified",
    lines_added: 3,
    lines_removed: 1,
    ...overrides,
  };
}

const sampleFiles: GitFileEntry[] = [
  file({ path: "src/main.rs", change: "modified", lines_added: 3, lines_removed: 1 }),
  file({ path: "src/lib.rs", change: "added", lines_added: 42, lines_removed: 0 }),
  file({ path: "Cargo.toml", change: "modified", lines_added: 2, lines_removed: 2 }),
  file({ path: "old.txt", change: "deleted", lines_added: 0, lines_removed: 10 }),
];

const sampleBranches = [
  { name: "main", current: true },
  { name: "feature/foo", current: false },
];

/** Set up invoke mock to dispatch by command name. */
function mockInvoke(opts: {
  diff?: GitFileEntry[] | Error;
  branches?: { name: string; current: boolean }[];
  fileDiff?: string | Error;
  commit?: string | Error;
} = {}) {
  invokeMock.mockImplementation(async (cmd: string, args?: any) => {
    switch (cmd) {
      case "cmd_get_git_diff": {
        const val = opts.diff ?? sampleFiles;
        if (val instanceof Error) throw val;
        return JSON.stringify(val);
      }
      case "cmd_list_branches": {
        const val = opts.branches ?? sampleBranches;
        return JSON.stringify(val);
      }
      case "cmd_get_git_file_diff": {
        const val = opts.fileDiff;
        if (val instanceof Error) throw val;
        return val ?? "";
      }
      case "cmd_git_commit": {
        const val = opts.commit;
        if (val instanceof Error) throw val;
        return val ?? "ok";
      }
      case "cmd_switch_branch":
        return "switched";
      default:
        return undefined;
    }
  });
}

function setup() {
  const i18n = createI18n("zh");
  const host = document.createElement("div");
  document.body.append(host);

  const [open, setOpen] = createSignal(true);
  const onClose = vi.fn(() => setOpen(false));

  const dispose = render(
    () => (
      <I18nCtx.Provider value={i18n}>
        <GitDiffPanel open={open()} seed="test-seed" onClose={onClose} />
      </I18nCtx.Provider>
    ),
    host,
  );

  return { host, dispose, open, onClose };
}

function flush() {
  return new Promise((r) => setTimeout(r, 40));
}

describe("GitDiffPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("1. renders file list with change types, paths, and stats", async () => {
    mockInvoke();

    const { host, dispose } = setup();
    await flush();

    const text = host.textContent ?? "";
    expect(text).toContain("src/main.rs");
    expect(text).toContain("src/lib.rs");
    expect(text).toContain("Cargo.toml");
    expect(text).toContain("old.txt");
    expect(text).toContain("+3");
    expect(text).toContain("-1");
    expect(text).toContain("+42");

    dispose();
    host.remove();
  });

  it("2. clicking a file calls cmd_get_git_file_diff", async () => {
    mockInvoke({ fileDiff: "<div>diff</div>" });

    const { host, dispose } = setup();
    await flush();

    const fileRows = host.querySelectorAll(".git-file-item");
    expect(fileRows.length).toBeGreaterThan(0);
    (fileRows[0] as HTMLElement).click();
    await flush();

    const diffCalls = invokeMock.mock.calls.filter(
      (c) => c[0] === "cmd_get_git_file_diff",
    );
    expect(diffCalls.length).toBeGreaterThanOrEqual(1);
    expect(diffCalls[0]?.[1]).toMatchObject({
      seed: "test-seed",
      filePath: "src/main.rs",
    });

    dispose();
    host.remove();
  });

  it("3. load failure shows empty state, not blank", async () => {
    mockInvoke({ diff: new Error("not a git repository"), branches: [] });

    const { host, dispose } = setup();
    await flush();

    const hasFeedback =
      host.textContent?.includes("没有变更") ||
      host.querySelector(".git-workspace-empty") !== null;
    expect(hasFeedback).toBe(true);

    dispose();
    host.remove();
  });

  it("4. commit submit is disabled when message is empty", async () => {
    mockInvoke();

    const { host, dispose } = setup();
    await flush();

    const submitBtn = host.querySelector<HTMLButtonElement>(".git-commit-submit");
    expect(submitBtn).toBeTruthy();
    expect(submitBtn!.disabled).toBe(true);

    dispose();
    host.remove();
  });

  it("5. does NOT show stage/unstage/discard/reset buttons", async () => {
    mockInvoke();

    const { host, dispose } = setup();
    await flush();

    const text = host.textContent ?? "";
    expect(text).not.toContain("stage");
    expect(text).not.toContain("unstage");
    expect(text).not.toContain("暂存");

    const standaloneDiscard = [...host.querySelectorAll("button")].filter(
      (b) =>
        (b.textContent?.includes("discard") || b.textContent?.includes("丢弃")) &&
        !b.textContent?.includes("切换"),
    );
    expect(standaloneDiscard.length).toBe(0);

    const resetBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.includes("reset") || b.textContent?.includes("Reset"),
    );
    expect(resetBtn).toBeUndefined();

    dispose();
    host.remove();
  });

  it("6. non-git workspace shows clear empty state", async () => {
    mockInvoke({ diff: new Error("not a git repository"), branches: [] });

    const { host, dispose } = setup();
    await flush();

    expect(host.textContent).toContain("没有变更");

    dispose();
    host.remove();
  });

  it("7. close button calls onClose", async () => {
    mockInvoke();

    const { host, dispose, onClose } = setup();
    await flush();

    const closeBtn = host.querySelector<HTMLButtonElement>(
      'button[aria-label="Close"]',
    );
    expect(closeBtn).toBeTruthy();
    closeBtn!.click();
    expect(onClose).toHaveBeenCalled();

    dispose();
    host.remove();
  });

  it("8. commit success calls cmd_git_commit with message", async () => {
    mockInvoke();

    const { host, dispose } = setup();
    await flush();

    const input = host.querySelector<HTMLInputElement>(".git-commit-input");
    expect(input).toBeTruthy();
    input!.value = "fix: update modules";
    input!.dispatchEvent(new Event("input", { bubbles: true }));
    await flush();

    const submitBtn = host.querySelector<HTMLButtonElement>(".git-commit-submit");
    expect(submitBtn).toBeTruthy();
    expect(submitBtn!.disabled).toBe(false);
    submitBtn!.click();
    await flush();
    await flush();

    const commitCalls = invokeMock.mock.calls.filter(
      (c) => c[0] === "cmd_git_commit",
    );
    expect(commitCalls.length).toBeGreaterThanOrEqual(1);
    expect(commitCalls[0]?.[1]).toMatchObject({
      seed: "test-seed",
      message: "fix: update modules",
    });

    dispose();
    host.remove();
  });

  it("9. when open=false the overlay is not rendered", async () => {
    mockInvoke();

    const i18n = createI18n("zh");
    const host = document.createElement("div");
    document.body.append(host);
    const [open, setOpen] = createSignal(false);

    const dispose = render(
      () => (
        <I18nCtx.Provider value={i18n}>
          <GitDiffPanel open={open()} seed="test-seed" onClose={() => {}} />
        </I18nCtx.Provider>
      ),
      host,
    );
    await flush();

    const overlay = host.querySelector(".git-workspace-overlay");
    expect(overlay).toBeNull();

    dispose();
    host.remove();
  });
});
