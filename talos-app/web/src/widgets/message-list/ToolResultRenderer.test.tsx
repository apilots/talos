import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import "@/shared/i18n/setup";
import { buildToolResultModel } from "./ToolResultRenderer";

describe("buildToolResultModel", () => {
  it("renders the saved Bash log reference and capture completeness on replay", () => {
    const model = buildToolResultModel({ contentItems: [{
      type: "bash_output", session_id: null, status: "failed", exit_code: 7,
      stdout: "FINAL ERROR\n", stderr: "", truncated: true, elapsed_ms: 100,
      outputLog: { path: "/workspace/user/tool-output/s/t/c/output.log", relativePath: "tool-output/s/t/c/output.log",
        totalBytes: 5000000, storedBytes: 5000000, totalLines: 300000, captureStatus: "complete", expiresAt: 1900000000 },
    }], progress: [], status: "error" });
    const html = renderToStaticMarkup(model.body);
    expect(html).toContain("FINAL ERROR");
    expect(html).toContain("/workspace/user/tool-output/s/t/c/output.log");
    expect(html).toContain("5000000 / 5000000 bytes saved");
    expect(html).toContain("complete");
  });

  it("renders byte continuation without inventing total line counts", () => {
    const model = buildToolResultModel({ contentItems: [{ type: "file_read", path: "/logs/output.log",
      offset: 0, limit: 0, total_lines: null, nextByteOffset: 4096, content: "fragment", truncated: true }],
      progress: [], status: "done" });
    const html = renderToStaticMarkup(model.body);
    expect(html).toContain("byte range");
    expect(html).toContain("byte_offset=4096");
    expect(html).not.toContain("NaN");
  });
  it("shows a final text error alongside the streamed output preview", () => {
    const model = buildToolResultModel({
      contentItems: [{ type: "text", text: "permission denied" }],
      progress: [{ kind: "stderr", message: "temporary file created\n" }],
      status: "error",
    });

    expect(model.summary).toBe("Failed");
    expect(model.preview).toEqual(["permission denied"]);
    const html = renderToStaticMarkup(model.body);
    expect(html).toContain('aria-label="Execution error"');
    expect(html).toContain("permission denied");
    expect(html).toContain('aria-label="Standard error (stderr) · streamed preview"');
    expect(html).toContain("temporary file created");
  });

  it("retains a streamed channel omitted by truncated Bash capture without appending partial duplicates", () => {
    const model = buildToolResultModel({ contentItems: [{ type: "bash_output", session_id: null,
      status: "completed", exit_code: 0, stdout: "retained stdout", stderr: "", truncated: true,
      elapsed_ms: 5, outputLog: null }],
      progress: [{ kind: "stdout", message: "old stdout" }, { kind: "stderr", message: "previous stderr" }],
      status: "done" });
    const html = renderToStaticMarkup(model.body);
    expect(html).toContain('aria-label="Standard output (stdout)"');
    expect(html).toContain('aria-label="Standard error (stderr) · streamed preview"');
    expect(html).toContain("The output shown here is truncated.");
    expect(html).toContain("retained stdout");
    expect(html).toContain("previous stderr");
    expect(html).not.toContain("old stdout");
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
        outputLog: null,
      }],
      progress: [],
      status: "done",
    });

    expect({ summary: model.summary, preview: model.preview }).toEqual({
      summary: "",
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
      preview: [],
    });
    const html = renderToStaticMarkup(model.body);
    expect(html).toContain("File changes");
    expect(html).toContain("src/app.tsx");
    expect(html).not.toContain("No error details were returned");
  });

  it("presents a structured subagent interrupt failure as an execution error", () => {
    const message = "Subagent cleanup has not been confirmed.";
    const model = buildToolResultModel({
      contentItems: [{ type: "json", value: { code: "runCleanupUnconfirmed", message, localization: null } }],
      progress: [], status: "error",
    });
    const html = renderToStaticMarkup(model.body);
    expect(model.preview).toEqual([message]);
    expect(html).toContain('aria-label="Execution error"');
    expect(html).toContain(message);
    expect(html).not.toContain("No error details were returned");
    expect(html).not.toContain("runCleanupUnconfirmed");
  });

  it("reports missing error details when no result or output is available", () => {
    const model = buildToolResultModel({ progress: [], status: "error" });
    expect(renderToStaticMarkup(model.body)).toContain("No error details were returned");
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
