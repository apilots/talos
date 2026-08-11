// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { ToolCallBlock } from "./ToolCallBlock";

describe("ToolCallBlock", () => {
  afterEach(cleanup);

  it("presents waiting status with complete tool details", () => {
    const { container } = render(
      <ToolCallBlock
        argumentsText={'{"cmd":"pnpm test"}'}
        callId="call-waiting"
        progress={[{ kind: "status", message: "Waiting for approval." }]}
        prompt="Approve this command."
        status="waiting_dynamic_input"
        timingMs={420}
        toolName="Bash"
      />,
    );

    const toggle = screen.getByRole("button", { name: /Bash · pnpm test/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("Waiting").textContent).toBe("Waiting");
    expect(screen.getByText("420ms").textContent).toBe("420ms");
    expect(screen.getByText("Waiting for input").textContent).toBe("Waiting for input");
    expect(screen.getByText("Approve this command.").textContent).toBe("Approve this command.");

    fireEvent.click(screen.getByText("Details"));

    expect(container.querySelector("details")?.open).toBe(true);
    expect(screen.getByText("call-waiting").textContent).toBe("call-waiting");
    expect(screen.getByText("Waiting for approval.").textContent).toBe("Waiting for approval.");
    expect(container.querySelector("pre")?.textContent).toBe('{"cmd":"pnpm test"}');
  });

  it("presents failed typed results with failure summary and result body", () => {
    render(
      <ToolCallBlock
        argumentsText={'{"path":"src/app.tsx"}'}
        callId="call-failed"
        contentItems={[{
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
              lines: [{ kind: "added", text: "new line" }],
            }],
          }],
        }]}
        progress={[]}
        status="error"
        toolName="Edit"
      />,
    );

    expect(screen.getByText("Failed").textContent).toBe("Failed");
    expect(screen.getByText("Failed · 1 file(s) · +1 -0").textContent).toBe(
      "Failed · 1 file(s) · +1 -0",
    );

    const toggle = screen.getByRole("button", { name: /Edit · src\/app\.tsx/ });
    fireEvent.click(toggle);

    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    const resultBody = screen.getByText(
      (_, element) => element?.tagName === "PRE" && element.textContent?.includes("+new line") === true,
    );
    expect(resultBody.textContent).toContain("+new line");
  });

  it("separates completed status, item duration, and Bash result summary", () => {
    render(
      <ToolCallBlock
        argumentsText={'{"cmd":"pnpm test"}'}
        callId="call-completed"
        contentItems={[{
          type: "bash_output",
          session_id: null,
          status: "completed",
          exit_code: 0,
          stdout: "tests passed\n",
          stderr: "",
          truncated: false,
          elapsed_ms: 240,
        }]}
        progress={[]}
        status="done"
        timingMs={300}
        toolName="Bash"
      />,
    );

    expect(screen.getByText("Completed").textContent).toBe("Completed");
    expect(screen.getByText("300ms").textContent).toBe("300ms");
    expect(screen.getByText("Completed · exit 0").textContent).toBe(
      "Completed · exit 0",
    );
    expect(screen.getByText("tests passed").textContent).toBe("tests passed");
  });

  it("presents a completed timeline tool as a plain execution step with compact result metadata", () => {
    const { container } = render(
      <ToolCallBlock
        argumentsText={'{"pattern":"src/**/*.ts"}'}
        callId="call-search"
        contentItems={[{
          type: "search_results",
          query: "src/**/*.ts",
          matches: [{
            path: "src/app.ts",
            line: 12,
            column: 1,
            text: "export const app = true;",
            before: [],
            after: [],
          }],
          offset: 0,
          limit: 10,
          total_matches: 1,
          more_available: false,
          partial: false,
        }]}
        progress={[]}
        status="done"
        timingMs={240}
        toolName="Glob"
        variant="timeline"
      />,
    );

    expect(container.querySelector('[data-tool-surface="plain"]')).not.toBeNull();
    expect(container.querySelector('[data-tool-status-presentation="inline"]')?.textContent).toContain("Completed");
    expect(container.querySelector('[data-tool-result-preview="compact"]')?.textContent).toContain("1 result(s)");
    expect(screen.getByText("240ms").textContent).toBe("240ms");
  });
});
