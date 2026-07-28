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

  it("renders server-owned subagent result without deriving lifecycle", () => {
    const model = buildToolResultModel({
      contentItems: [{
        type: "subagent_result",
        result: {
          operation: "waitAgent",
          result: {
            subagent: {
              path: "/root/review",
              sessionId: "child-session",
              parent: {
                sessionId: "root-session",
                turnId: "root-turn",
              },
            },
            agentId: "reviewer",
            latestUpdate: null,
            outcome: {
              status: "completed",
              turnId: "child-turn",
              output: "review complete",
              truncated: false,
            },
          },
        },
      }],
      progress: [],
      status: "done",
    });

    expect({ summary: model.summary, preview: model.preview }).toEqual({
      summary: "Completed · /root/review · completed",
      preview: ["agent: reviewer", "session: child-session"],
    });
  });

  it("renders the server-owned send-message acknowledgement", () => {
    const model = buildToolResultModel({
      contentItems: [{
        type: "subagent_result",
        result: {
          operation: "sendMessage",
          result: {
            subagent: {
              path: "/root/review",
              sessionId: "child-session",
              parent: {
                sessionId: "root-session",
                turnId: "root-turn",
              },
            },
            agentId: "reviewer",
            turn: {
              sessionId: "child-session",
              turnId: "child-turn",
            },
            accepted: true,
          },
        },
      }],
      progress: [],
      status: "done",
    });

    expect({ summary: model.summary, preview: model.preview }).toEqual({
      summary: "Completed · Message accepted by /root/review",
      preview: ["agent: /root/review", "agent id: reviewer"],
    });
  });
});
