import { describe, expect, it } from "vitest";
import { workspaceDisplayPath } from "./workspacePath";

describe("workspaceDisplayPath", () => {
  it("uses a Unix-style path relative to the workspace", () => {
    expect(workspaceDisplayPath("D:\\DeepX\\crates\\deepx-tauri\\src\\App.tsx", "D:\\DeepX"))
      .toBe("crates/deepx-tauri/src/App.tsx");
  });

  it("compacts paths outside the workspace", () => {
    expect(workspaceDisplayPath("C:\\Users\\agent\\cache\\nested\\file.ts", "D:\\DeepX"))
      .toBe("…/cache/nested/file.ts");
  });
});
