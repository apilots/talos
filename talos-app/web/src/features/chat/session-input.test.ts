import { describe, expect, it } from "vitest";

import { buildQueuedInput } from "./session-input";

describe("buildQueuedInput", () => {
  it("keeps Skill mentions in the text item for backend resolution", () => {
    expect(buildQueuedInput("use $browser")).toEqual([
      { type: "text", text: "use $browser" },
    ]);
  });

  it("keeps file references separate from text", () => {
    expect(buildQueuedInput("inspect this", { fileIds: ["file-2", "file-1"] })).toEqual([
      { type: "text", text: "inspect this" },
      { type: "files", fileIds: ["file-2", "file-1"] },
    ]);
  });
});
