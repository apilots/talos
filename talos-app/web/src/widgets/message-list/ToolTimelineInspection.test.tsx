import { createElement } from "react";
import type { ChatRenderBlock } from "@/shared/types/ui";
// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import type { ChatTimelineItem } from "@/shared/types/ui";
import { MessageList } from "./MessageList";
import { ToolCallBlock } from "./ToolCallBlock";

describe("Tool timeline inspection", () => {
  afterEach(cleanup);

  it("keeps an inspected live tool and its details open when the turn settles", () => {
    const command: Extract<ChatTimelineItem, { kind: "tool_call" }> = {
      itemId: "tool-1", kind: "tool_call", phase: "acting", status: "running",
      callId: "call-1", toolName: "Bash", arguments: { command: "sleep 30" },
      progress: [{ kind: "stdout", message: "live stream" }],
      startedAtMs: 1_000,
    };
    const view = createElement<{ blocks: Extract<ChatRenderBlock, { kind: "timeline" }>[]; emptyDetail: string; emptyTitle: string }>(MessageList, {
blocks: [{ id: "timeline-inspected", kind: "timeline", live: true, items: [command] }],
emptyDetail: "",
emptyTitle: ""
});
    const { rerender } = render(view);
    const tool = screen.getByRole("button", { name: /Bash.*sleep 30.*Running/ });
    fireEvent.click(tool);
    const disclosure = screen.getByText("Details");
    fireEvent.click(disclosure);
    disclosure.focus();
    expect(within(screen.getByRole("region", { name: "Live output" })).getByText("live stream")).toBeTruthy();

    rerender(<MessageList {...view.props} blocks={[{
      ...view.props.blocks[0], live: false,
      items: [{ ...command, status: "completed", completedAtMs: 31_000,
        contentItems: [{ type: "bash_output", session_id: null, status: "completed", exit_code: 0,
          stdout: "final stream", stderr: "", truncated: false, elapsed_ms: 30_000, outputLog: null }] }],
    }]} />);
    expect(screen.getByRole("button", { name: /Bash.*sleep 30.*Completed/ })).toBe(tool);
    expect(tool.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("Details")).toBe(disclosure);
    expect(disclosure.closest("details")?.open).toBe(true);
    expect(document.activeElement).toBe(disclosure);
    expect(within(screen.getByRole("region", { name: "Command result" })).getByText("final stream")).toBeTruthy();
  });

  it("does not mark previously streamed output as live on an inactive card", () => {
    render(<ToolCallBlock argumentsValue={{}} callId="call-replay" toolName="Fixture" variant="timeline"
      live={false} progress={[{ kind: "stderr", message: "previous chunk" }]} status="running" />);
    fireEvent.click(screen.getByRole("button", { name: /Fixture.*Last seen running/ }));
    expect(within(screen.getByRole("region", { name: "Streamed output" })).getByText("previous chunk")).toBeTruthy();
    fireEvent.click(screen.getByText("Details"));
    const details = within(screen.getByText("Details").closest("details")!);
    expect(details.getByRole("region", { name: "Arguments" })).toBeTruthy();
    expect(details.queryByRole("region", { name: "Execution progress" })).toBeNull();
  });
});
