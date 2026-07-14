import { createRoot } from "solid-js";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@tauri-apps/api/core";
import { createChatStore } from "./chat";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

function ask(ask_id: string, question: string) {
  return {
    ask_id,
    mode: "single",
    questions: [{ id: "q1", question, options: ["yes"], allow_custom: false }],
  };
}

describe("ask_user frontend lifecycle", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it("queues asks and keeps the active form until the matching ACK", async () => {
    await createRoot(async (dispose) => {
      const chat = createChatStore("seed-1");
      chat.showAskDialog(ask("ask-1", "First?"));
      chat.showAskDialog(ask("ask-2", "Second?"));

      expect(chat.askState().askId).toBe("ask-1");
      await chat.submitAskAnswer([{ question_id: "q1", answer: "yes" }]);
      await chat.submitAskAnswer([{ question_id: "q1", answer: "yes" }]);

      expect(invokeMock).toHaveBeenCalledTimes(1);
      expect(chat.askState()).toMatchObject({ askId: "ask-1", show: true });

      chat.handleAskResolved("stale");
      expect(chat.askState().askId).toBe("ask-1");
      chat.handleAskResolved("ask-1");
      expect(chat.askState()).toMatchObject({ askId: "ask-2", show: true });
      dispose();
    });
  });

  it("keeps a rejected ask active and allows a corrected retry", async () => {
    await createRoot(async (dispose) => {
      const chat = createChatStore("seed-1");
      chat.showAskDialog(ask("ask-1", "First?"));
      await chat.submitAskAnswer([{ question_id: "q1", answer: "bad" }]);
      chat.handleAskRejected("ask-1", "invalid option");

      expect(chat.askState()).toMatchObject({ askId: "ask-1", show: true });
      expect(chat.error()).toBe("invalid option");
      await chat.submitAskAnswer([{ question_id: "q1", answer: "yes" }]);
      expect(invokeMock).toHaveBeenCalledTimes(2);
      dispose();
    });
  });

  it("ignores ask events without an authoritative ask_id", () => {
    createRoot((dispose) => {
      const chat = createChatStore("seed-1");
      chat.showAskDialog({ mode: "single", questions: [] });
      expect(chat.askState().show).toBe(false);
      dispose();
    });
  });

  it("invalidates active and queued asks on cancellation, session switch, and undo", async () => {
    await createRoot(async (dispose) => {
      const chat = createChatStore("seed-1");
      chat.showAskDialog(ask("ask-1", "First?"));
      chat.showAskDialog(ask("ask-2", "Second?"));
      chat.handleCancelled();
      expect(chat.askState().show).toBe(false);

      chat.showAskDialog(ask("ask-3", "Third?"));
      expect(chat.askState().askId).toBe("ask-3");
      chat.handleSessionCreated("seed-2");
      expect(chat.askState().show).toBe(false);

      chat.showAskDialog(ask("ask-4", "Fourth?"));
      await chat.undoTurn("t4");
      expect(chat.askState().show).toBe(false);
      dispose();
    });
  });

  it("keeps the ask recoverable when undo delivery fails", async () => {
    await createRoot(async (dispose) => {
      const chat = createChatStore("seed-1");
      chat.showAskDialog(ask("ask-1", "First?"));
      invokeMock.mockRejectedValueOnce(new Error("agent unavailable"));
      await chat.undoTurn("t1");
      expect(chat.askState()).toMatchObject({ askId: "ask-1", show: true });
      dispose();
    });
  });
});
