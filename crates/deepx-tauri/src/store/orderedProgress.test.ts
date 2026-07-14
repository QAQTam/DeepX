import { describe, expect, it } from "vitest";
import { appendProgress, emptyProgress, materializeProgress } from "./orderedProgress";

describe("ordered progress", () => {
  it("orders stdout and stderr by seq without losing provenance", () => {
    let buffer = emptyProgress();
    buffer = appendProgress(buffer, { stream: "stdout", seq: 2, chunk: "B" });
    buffer = appendProgress(buffer, { stream: "stderr", seq: 1, chunk: "E" });
    expect(materializeProgress(buffer)).toEqual([
      { stream: "stderr", seq: 1, chunk: "E" },
      { stream: "stdout", seq: 2, chunk: "B" },
    ]);
  });

  it("replaces a repeated sequence instead of duplicating output", () => {
    let buffer = appendProgress(emptyProgress(), { stream: "stdout", seq: 1, chunk: "old" });
    buffer = appendProgress(buffer, { stream: "stderr", seq: 1, chunk: "new" });
    expect(materializeProgress(buffer)).toEqual([{ stream: "stderr", seq: 1, chunk: "new" }]);
  });
});
