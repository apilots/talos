import { describe, expect, it } from "vitest";

import { buildToolResultModel } from "./ToolResultRenderer";

describe("buildToolResultModel", () => {
  it("preserves error status for text result items", () => {
    const model = buildToolResultModel({
      contentItems: [{ type: "text", text: "permission denied" }],
      progress: [],
      status: "error",
    });

    expect(model.summary).toBe("Error");
    expect(model.preview).toEqual(["permission denied"]);
  });
});
