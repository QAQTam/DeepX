import { expect, it } from "vitest";
import { applyCodeDelta, environmentFromGit } from "./environmentStore";

it("combines initial git totals with later code deltas", () => {
  const initial = environmentFromGit([{ additions: 10, deletions: 3 }]);
  expect(applyCodeDelta(initial, { lines_added: 4, lines_removed: 2 }).changes).toEqual({ additions: 14, deletions: 5 });
});
