import { describe, expect, it } from "vitest";

import { classifyMemory } from "./useMemoryPage";

describe("useMemoryPage helpers", () => {
  it("classifies memory records into stable sections", () => {
    expect(
      classifyMemory({
        id: "1",
        title: "Code review preference",
        detail: "Prefer concise findings first.",
        tags: ["review"],
        updatedAt: Date.now(),
      }),
    ).toBe("workPreferences");

    expect(
      classifyMemory({
        id: "2",
        title: "Workspace convention",
        detail: "talos-app mirrors shared APIs first.",
        tags: ["workspace"],
        updatedAt: Date.now(),
      }),
    ).toBe("projectContext");
  });
});
