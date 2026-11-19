// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import type { ChatTimelineItem } from "@/shared/types/ui";
import { MessageList } from "@/widgets/message-list/MessageList";

const command: Extract<ChatTimelineItem, { kind: "tool_call" }> = {
  itemId: "tool-1",
  kind: "tool_call",
  phase: "acting",
  status: "running",
  callId: "call-1",
  toolName: "Bash",
  arguments: { command: "sleep 30" },
  progress: [{ kind: "status", message: "execution started" }],
  startedAtMs: 1_000,
};

describe("MessageTimelineBlock lifecycle", () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("keeps waiting for the next item only after the latest steer", () => {
    const view = (
      <MessageList
        blocks={[{
          id: "timeline-awaiting-next-item",
          kind: "timeline",
          live: true,
          items: [{ ...command, status: "completed", completedAtMs: 1_100 }],
        }]}
        emptyDetail=""
        emptyTitle=""
      />
    );

    const { rerender } = render(view);
    const previous = screen.getByRole("status");
    expect(previous.textContent).toBe("Waiting for the next update…");

    rerender(<MessageList {...view.props} blocks={[
      ...view.props.blocks,
      { id: "steer", kind: "user", text: "Continue", fileIds: [] },
      { ...view.props.blocks[0], id: "after-steer" },
    ]} />);
    expect(screen.getByRole("status")).not.toBe(previous);
  });

  it("keeps commentary in sequence and honors an explicit collapse during live updates", async () => {
    const view = (
      <MessageList
        blocks={[{
          id: "timeline-activity",
          kind: "timeline",
          live: true,
          items: [
            { ...command, status: "completed", completedAtMs: 1_100 },
            { itemId: "progress", kind: "assistant_text", phase: "commentary", status: "completed", text: "The first check passed.", startedAtMs: 1_101 },
            { ...command, itemId: "tool-2", arguments: { command: "printf ready" }, status: "completed", completedAtMs: 1_200 },
          ],
        }]}
        emptyDetail=""
        emptyTitle=""
      />
    );
    const { container, rerender } = render(view);
    const groups = container.querySelectorAll<HTMLDetailsElement>("details");
    const progress = screen.getByText("The first check passed.");
    expect(groups[0].compareDocumentPosition(progress) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(progress.compareDocumentPosition(groups[1]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(groups[0]).getByText("Bash · sleep 30")).toBeTruthy();
    expect(within(groups[0]).queryByRole("button")).toBeNull();
    const groupSummary = groups[0].querySelector("summary")!;
    fireEvent.click(groupSummary);
    expect(await within(groups[0]).findByRole("button", { name: /Bash · sleep 30/ })).toBeTruthy();
    fireEvent.click(groupSummary);
    expect(groups[0].open).toBe(false);

    rerender(<MessageList {...view.props} blocks={[{
      ...view.props.blocks[0],
      items: [...view.props.blocks[0].items, { ...command, itemId: "tool-3", arguments: { command: "sleep 20" } }],
    }]} />);
    expect(groups[0].open).toBe(false);
    expect(screen.getByText("The first check passed.").closest("details")).toBeNull();
    expect(screen.getByText(/sleep 20/).closest("details")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^Working/ }));
    expect(screen.queryByText("Bash · sleep 30")).toBeNull();
    expect(screen.queryByText("The first check passed.")).toBeNull();
    expect(screen.queryByText(/sleep 20/)).toBeNull();
    expect(container.querySelector("[data-process-surface]")).toBeNull();

    const nextItems = [...view.props.blocks[0].items,
      { ...command, itemId: "tool-3", arguments: { command: "sleep 20" } },
      { itemId: "next-progress", kind: "assistant_text", phase: "commentary", status: "running",
        text: "The second check is ready.", startedAtMs: 1_301 } satisfies ChatTimelineItem,
    ];
    rerender(<MessageList {...view.props} blocks={[{ ...view.props.blocks[0], items: nextItems }]} />);
    expect(screen.queryByText("The second check is ready.")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^Working/ }));
    const resumedTool = screen.getByText(/sleep 20/);
    const resumedProgress = screen.getByText("The second check is ready.");
    expect(resumedTool.compareDocumentPosition(resumedProgress) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByText("The first check passed.")).toBeTruthy();
  });

  it("ticks while Bash is silent, exposes the latest output, and pauses live activity when disconnected", () => {
    vi.useFakeTimers();
    vi.setSystemTime(121_000);
    const view = (
      <MessageList blocks={[{ id: "timeline-running", kind: "timeline", live: true, items: [command] }]}
        emptyDetail="" emptyTitle="" />
    );

    const { container, rerender } = render(view);
    expect(screen.getByText("Running")).toBeTruthy();
    expect(screen.getByText("No command output yet")).toBeTruthy();
    expect(screen.getByText("2m00s")).toBeTruthy();
    expect(screen.getByText("Working for 2m00s")).toBeTruthy();
    const status = screen.getByRole("status");
    expect(status.textContent).toBe("Working");
    expect(status.parentElement?.parentElement?.querySelector("svg")?.classList.contains("motion-safe:animate-spin")).toBe(true);
    act(() => vi.advanceTimersByTime(2_000));
    expect(screen.getByText("2m02s")).toBeTruthy();
    expect(screen.getByText("Working for 2m02s")).toBeTruthy();

    const reportingCommand: typeof command = {
      ...command,
      progress: [...command.progress, { kind: "stdout", message: "started\nlatest output\n" }],
    };
    const reportingBlocks = [{ ...view.props.blocks[0], items: [reportingCommand] }];
    rerender(<MessageList {...view.props} blocks={reportingBlocks} />);
    expect(screen.getByText("latest output")).toBeTruthy();
    expect(screen.queryByText("No command output yet")).toBeNull();
    expect(screen.queryByText("started")).toBeNull();

    rerender(<MessageList {...view.props} blocks={reportingBlocks} liveUpdatesPaused />);
    expect(screen.getByText("Last seen running")).toBeTruthy();
    expect(container.querySelector("[data-tool-duration]")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();

    rerender(<MessageList {...view.props} blocks={[{
      ...view.props.blocks[0],
      items: [{ ...reportingCommand, status: "completed", completedAtMs: 123_000 }],
    }]} />);
    expect(screen.getByText("Bash · sleep 30")).toBeTruthy();
    expect(screen.queryByText("Running")).toBeNull();
    expect(screen.getByRole("status").textContent).toBe("Waiting for the next update…");
  });

  it("keeps Thinking before its command and only groups consecutive successful tools", async () => {
    const { container } = render(
      <MessageList blocks={[{ id: "ordered", kind: "timeline", live: true, items: [
        { kind: "reasoning", phase: "thinking", status: "completed", itemId: "before",
          text: "Choose the first check.", startedAtMs: 1_000, completedAtMs: 1_050 },
        { ...command, arguments: { command: "sleep 30\nprintf ready", cmd: "printf other" }, output: "command output",
          status: "completed", completedAtMs: 1_100 },
        { ...command, itemId: "read", toolName: "Read", arguments: { path: "reports/result.txt", file_path: "other.txt" },
          status: "completed", startedAtMs: 1_100, completedAtMs: 1_190 },
        { kind: "reasoning", phase: "thinking", status: "completed", itemId: "between",
          text: "Verify the read result.", startedAtMs: 1_200, completedAtMs: 1_250 },
        { ...command, itemId: "next", arguments: { command: "printf checked" },
          status: "completed", startedAtMs: 1_300, completedAtMs: 1_400 },
      ] }]} emptyDetail="" emptyTitle="" />,
    );
    const groups = container.querySelectorAll<HTMLDetailsElement>("[data-timeline-activity]");
    expect(Array.from(groups, (group) => group.querySelector("summary > span")?.textContent)).toEqual([
      "Thinking", "Ran commands and Read files", "Thinking", "Bash · printf checked",
    ]);
    const tools = within(groups[1]);
    expect(tools.queryByText(/sleep 30|result\.txt|command output/)).toBeNull();
    fireEvent.click(tools.getByText("Ran commands and Read files"));
    const bashRow = await tools.findByRole("button", { name: "Bash · sleep 30 printf ready" });
    expect(tools.getByRole("button", { name: "Read result.txt" })).toBeTruthy();
    expect(tools.queryByText("command output")).toBeNull();
    expect(tools.queryByText("Thinking")).toBeNull();
    fireEvent.click(bashRow);
    expect(tools.getByText("Command").nextElementSibling?.textContent).toBe("sleep 30\nprintf ready");
    expect(tools.getByText("command output")).toBeTruthy();
    fireEvent.click(within(groups[2]).getByText("Thinking"));
    expect(await within(groups[2]).findByText("Verify the read result.")).toBeTruthy();
  });

  it("nests Thinking only within verified Bash execution", async () => {
    const { container } = render(
      <MessageList blocks={[{ id: "overlap", kind: "timeline", live: true, items: [
        { ...command, status: "completed", completedAtMs: 7_000, executionMetrics: {
          queuedAtMs: 1_000, startedAtMs: 2_100, finishedAtMs: 6_500, queueDurationMs: 1_100, executionDurationMs: 4_400,
        } },
        { kind: "reasoning", phase: "thinking", status: "completed", itemId: "inside",
          text: "Think while Bash runs.", startedAtMs: 3_000, completedAtMs: 4_000 },
        { kind: "reasoning", phase: "thinking", status: "completed", itemId: "uncertain",
          text: "The completion second straddles the command ending.", startedAtMs: 6_000, completedAtMs: 6_000 },
      ] }]} emptyDetail="" emptyTitle="" />,
    );
    const groups = container.querySelectorAll<HTMLDetailsElement>("[data-timeline-activity]");
    expect(groups).toHaveLength(2);
    expect(within(groups[1]).getByText("Thinking")).toBeTruthy();
    fireEvent.click(within(groups[0]).getByText("Ran commands"));
    const thinking = await within(groups[0]).findByText("Thinking");
    expect(thinking.closest("details")?.open).toBe(false);
    fireEvent.click(thinking);
    expect(within(groups[0]).getByText("Think while Bash runs.")).toBeTruthy();
    expect(within(groups[0]).queryByText(/straddles/)).toBeNull();
  });

  it("summarizes unclassified tools once in a mixed group and shows their names when opened", async () => {
    const { container } = render(
      <MessageList blocks={[{ id: "mixed-tools", kind: "timeline", live: true, items: [
        { ...command, toolName: "Read", arguments: { path: "result.txt" }, status: "completed" },
        { ...command, itemId: "todo", callId: "todo", toolName: "TodoWrite", arguments: {}, status: "completed" },
        { ...command, itemId: "mcp", callId: "mcp", toolName: "mcp__example__inspect", arguments: {}, status: "completed" },
      ] }]} emptyDetail="" emptyTitle="" />,
    );
    const group = within(container.querySelector("[data-timeline-activity]")! as HTMLElement);
    const summary = group.getByText("Read files and Other tools", { exact: true });
    expect(group.queryByRole("button")).toBeNull();
    fireEvent.click(summary);
    expect(await group.findByRole("button", { name: "TodoWrite" })).toBeTruthy();
    expect(group.getByRole("button", { name: "mcp__example__inspect" })).toBeTruthy();
  });
});
