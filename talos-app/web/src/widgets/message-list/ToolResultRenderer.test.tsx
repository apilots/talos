import { describe, expect, it } from "vitest";

import { buildToolResultModel } from "./ToolResultRenderer";

describe("buildToolResultModel", () => {
  it("preserves error status for text result items", () => {
    const model = buildToolResultModel({
      contentItems: [{ type: "text", text: "permission denied" }],
      progress: [],
      status: "error",
    });

    expect(model.summary).toBe("Failed");
    expect(model.preview).toEqual(["permission denied"]);
  });

  it("summarizes typed Bash output", () => {
    const model = buildToolResultModel({
      contentItems: [{
        type: "bash_output",
        session_id: null,
        status: "completed",
        exit_code: 0,
        stdout: "tests passed\n",
        stderr: "",
        truncated: false,
        elapsed_ms: 240,
      }],
      progress: [],
      status: "done",
    });

    expect({ summary: model.summary, preview: model.preview }).toEqual({
      summary: "Completed · exit 0",
      preview: ["tests passed"],
    });
  });

  it("summarizes typed diff output", () => {
    const model = buildToolResultModel({
      contentItems: [{
        type: "diff",
        files: [{
          path: "src/app.tsx",
          old_path: null,
          truncated: false,
          hunks: [{
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: [
              { kind: "removed", text: "old" },
              { kind: "added", text: "new" },
            ],
          }],
        }],
      }],
      progress: [],
      status: "error",
    });

    expect({ summary: model.summary, preview: model.preview }).toEqual({
      summary: "Failed · 1 file(s) · +1 -1",
      preview: ["1 file(s) changed · +1 -1"],
    });
  });
});
