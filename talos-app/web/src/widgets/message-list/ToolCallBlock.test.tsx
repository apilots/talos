// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { ToolCallBlock } from "./ToolCallBlock";

describe("ToolCallBlock", () => {
  afterEach(cleanup);

  it("presents waiting status with complete tool details", () => {
    const { container } = render(
      <ToolCallBlock
        argumentsValue={'{"cmd":"pnpm test"}'}
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
    expect(within(screen.getByRole("region", { name: "Execution progress" })).getByRole("listitem").textContent)
      .toBe("Waiting for approval.");
    expect(screen.getByRole("region", { name: "Arguments" }).querySelector("pre")?.textContent)
      .toBe('{"cmd":"pnpm test"}');
  });

  it("separates streamed channels and status, then replaces streams with the returned result", () => {
    const props = {
      argumentsValue: { command: "fixture" }, callId: "call-stream", toolName: "Bash", variant: "timeline",
      progress: [
        { kind: "status", message: "Execution started" },
        { kind: "status", message: "Executing Bash" },
        { kind: "status", message: "waiting for execution slot" },
        { kind: "status", message: "Executing a fallback step." },
        { kind: "status", message: "Some output was omitted." },
        { kind: "stdout", message: "stream out\n" },
        { kind: "stderr", message: "stream err\n" },
      ],
    } as const;
    const { rerender } = render(<ToolCallBlock {...props} progress={[...props.progress]} status="running" />);
    fireEvent.click(screen.getByRole("button", { name: /Bash.*Running/ }));
    fireEvent.click(screen.getByText("Details"));
    const live = within(screen.getByRole("region", { name: "Live output" }));
    expect(live.getByRole("region", { name: "Standard output (stdout)" }).querySelector("pre")?.textContent)
      .toBe("stream out\n");
    expect(live.getByRole("region", { name: "Standard error (stderr)" }).querySelector("pre")?.textContent)
      .toBe("stream err\n");
    expect(within(screen.getByRole("region", { name: "Execution progress" })).getAllByRole("listitem")
      .map((item) => item.textContent)).toEqual(["Executing a fallback step.", "Some output was omitted."]);
    expect(screen.getAllByText("stream out")).toHaveLength(1);

    rerender(<ToolCallBlock {...props} progress={[...props.progress]} status="done" contentItems={[{
      type: "bash_output", session_id: null, status: "completed", exit_code: 0,
      stdout: "final out\n", stderr: "", truncated: false, elapsed_ms: 300, outputLog: null,
    }]} />);
    const result = within(screen.getByRole("region", { name: "Command result" }));
    expect(result.getByRole("region", { name: "Standard output (stdout)" }).querySelector("pre")?.textContent)
      .toBe("final out\n");
    expect(screen.queryByText("stream out")).toBeNull();
    expect(screen.queryByText("stream err")).toBeNull();
    expect(screen.getByRole("button", { name: /Bash.*Completed/ })).toBeTruthy();
    expect(within(screen.getByRole("region", { name: "Execution progress" })).getAllByRole("listitem")
      .map((item) => item.textContent)).toEqual(["Executing a fallback step.", "Some output was omitted."]);
  });

  it.each([
    ["done", "Completed"], ["error", "Failed"], ["cancelled", "Cancelled"],
  ] as const)("shows the %s outcome only in the card header for lifecycle-only progress", (status, label) => {
    render(<ToolCallBlock argumentsValue={{}} callId="call-terminal" toolName="Grep" variant="activity"
      live={false} progress={[
        { kind: "status", message: "Executing Grep" }, { kind: "status", message: "execution started" },
      ]} status={status} />);
    const header = screen.getByRole("button", { name: "Searched files" });
    fireEvent.click(header);
    fireEvent.click(screen.getByText("Details"));
    expect(within(header).getByText(label)).toBeTruthy();
    const details = screen.getByText("Details").closest("details")!;
    expect(within(details).getByRole("region", { name: "Arguments" })).toBeTruthy();
    expect(within(details).queryByRole("list")).toBeNull();
  });

  it("shows a returned Bash cancellation as cancelled while preserving the result explanation", () => {
    render(<ToolCallBlock argumentsValue={{ command: "sleep 30" }} callId="call-cancelled" toolName="Bash"
      live={false} variant="timeline" progress={[{ kind: "status", message: "execution started" }]}
      status="error" contentItems={[{
        type: "bash_output", session_id: null, status: "cancelled", exit_code: null,
        stdout: "partial output", stderr: "", truncated: false, elapsed_ms: 100, outputLog: null,
      }]} />);
    fireEvent.click(screen.getByRole("button", { name: /Bash.*Cancelled/ }));
    fireEvent.click(screen.getByText("Details"));
    expect(within(screen.getByText("Details").closest("details")!).queryByRole("list")).toBeNull();
    const result = within(screen.getByRole("region", { name: "Command result" }));
    expect(result.getByText("The command was cancelled.")).toBeTruthy();
    expect(result.getByText("partial output")).toBeTruthy();
  });

  it("presents failed typed results with failure summary and result body", () => {
    render(
      <ToolCallBlock
        argumentsValue={'{"path":"src/app.tsx"}'}
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
    const result = screen.getByRole("region", { name: "Tool result" });
    expect(within(result).queryByRole("region", { name: "Execution error" })).toBeNull();
    const resultBody = within(within(result).getByRole("region", { name: "File changes" })).getByText(
      (_, element) => element?.tagName === "PRE" && element.textContent?.includes("+new line") === true,
    );
    expect(resultBody.textContent).toContain("+new line");
  });

  it("separates completed status, item duration, and Bash result summary", () => {
    render(
      <ToolCallBlock
        argumentsValue={'{"cmd":"pnpm test"}'}
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
          outputLog: null,
        }]}
        progress={[]}
        status="done"
        timingMs={300}
        toolName="Bash"
      />,
    );

    expect(screen.getByText("Completed").textContent).toBe("Completed");
    expect(screen.getByText("300ms").textContent).toBe("300ms");
    expect(screen.getByText("Exit 0").textContent).toBe("Exit 0");
    expect(screen.getByText("tests passed").textContent).toBe("tests passed");
  });

  it("shows one collapsed Bash error line and separates the expanded failure and output channels", () => {
    const { container } = render(<ToolCallBlock
      argumentsValue={{ command: "run-fixture" }}
      callId="failed-command"
      contentItems={[
        { type: "bash_output", session_id: null, status: "failed", exit_code: 23,
          stdout: "normal setup\nnormal done\n", stderr: "stderr banner\nmissing executable\n\n",
          truncated: false, elapsed_ms: 4_116, outputLog: null },
      ]}
      progress={[]}
      status="error"
      variant="timeline"
      toolName="Bash"
    />);
    const toggle = screen.getByRole("button", { name: /Bash.*Exit 23.*4\.1s.*Failed/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(container.querySelector("[data-tool-result-preview]")?.textContent).toBe("missing executable");
    expect(screen.queryByText("normal setup", { exact: false })).toBeNull();

    fireEvent.click(toggle);
    expect(screen.getByRole("region", { name: "Execution error" }).querySelector("pre")?.textContent)
      .toBe("The command did not exit successfully.");
    expect(screen.getByRole("region", { name: "Standard output (stdout)" }).querySelector("pre")?.textContent)
      .toBe("normal setup\nnormal done\n");
    expect(screen.getByRole("region", { name: "Standard error (stderr)" }).querySelector("pre")?.textContent)
      .toBe("stderr banner\nmissing executable\n\n");
    expect(screen.getByText("Command")).toBeTruthy();
    expect(container.querySelector("[data-tool-result-preview]")).toBeNull();
  });

  it("does not let stdout disguise a timeout in the failed preview", () => {
    const { container } = render(<ToolCallBlock
      argumentsValue={{ command: "fixture-timeout" }}
      callId="timeout-command"
      contentItems={[{ type: "bash_output", session_id: null, status: "timed_out", exit_code: null,
        stdout: "All checks passed", stderr: "", truncated: false, elapsed_ms: 1_000, outputLog: null }]}
      progress={[]}
      status="error"
      variant="timeline"
      toolName="Bash"
    />);
    expect(container.querySelector("[data-tool-result-preview]")?.textContent).toBe("The command exceeded its timeout.");
    expect(screen.queryByText("All checks passed")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Bash.*Failed/ }));
    expect(screen.getByRole("region", { name: "Execution error" }).querySelector("pre")?.textContent)
      .toBe("The command exceeded its timeout.");
    expect(screen.getByRole("region", { name: "Standard output (stdout)" }).querySelector("pre")?.textContent)
      .toBe("All checks passed");
  });

  it("presents a completed timeline tool as a plain execution step with compact result metadata", () => {
    const { container } = render(
      <ToolCallBlock
        argumentsValue={'{"pattern":"src/**/*.ts"}'}
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

  it("shows a WebFetch source once and keeps the fetched text available in its details", () => {
    const url = "https://api.open-meteo.com/v1/forecast";
    const responseText = '{"current":{"temperature_2m":4.2}}';
    render(<ToolCallBlock
      argumentsValue={{ url }}
      callId="web-weather"
      contentItems={[
        { type: "web_reference", url, title: url, source_domain: null, snippet: null,
          citation_id: "weather", published_at: null, provider_rank: null },
        { type: "text", text: responseText },
      ]}
      progress={[]}
      status="done"
      toolName="WebFetch"
      variant="timeline"
    />);
    expect(screen.queryByText(url, { exact: true })).toBeNull();
    expect(screen.queryByText(responseText, { exact: true })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /WebFetch/ }));
    expect(screen.getAllByText(url, { exact: true })).toHaveLength(1);
    expect(within(screen.getByRole("region", { name: "Web result" })).getByText(responseText, { exact: true })).toBeTruthy();
  });

  it("keeps multiple returned types in distinct sections of the same card", () => {
    render(<ToolCallBlock argumentsValue={{}} callId="mixed" toolName="Fixture" status="done" progress={[]}
      contentItems={[
        { type: "file_read", path: "sample.txt", content: "file value", offset: 0, limit: 1,
          total_lines: 1, nextByteOffset: null, truncated: false },
        { type: "json", value: { extra: "structured value" } },
      ]} />);
    fireEvent.click(screen.getByRole("button", { name: /Fixture.*Completed/ }));
    const result = within(screen.getByRole("region", { name: "Tool result" }));
    expect(result.getByRole("region", { name: "File content" }).textContent).toContain("file value");
    expect(result.getByRole("region", { name: "JSON response" }).textContent).toContain("structured value");
  });

  it("uses the returned content rather than the tool name to label JSON and references", () => {
    const props = { argumentsValue: {}, callId: "tool-typed", progress: [], status: "done", toolName: "Read" } as const;
    const { rerender } = render(<ToolCallBlock {...props} progress={[]} contentItems={[
      { type: "json", value: { result: "fixture" } },
    ]} />);
    fireEvent.click(screen.getByRole("button", { name: /Read.*Completed/ }));
    expect(screen.getByRole("region", { name: "JSON response" }).querySelector("pre")?.textContent)
      .toBe('{\n  "result": "fixture"\n}');
    rerender(<ToolCallBlock {...props} progress={[]} contentItems={[
      { type: "mcp_resource_ref", server: "fixture", uri: "file:///sample", mime_type: null },
    ]} />);
    expect(screen.getByRole("region", { name: "Returned references" }).querySelector("pre")?.textContent)
      .toBe("mcp: fixture:file:///sample");
  });
});
