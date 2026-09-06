// @vitest-environment jsdom

import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { MessageList } from "@/widgets/message-list/MessageList";

describe("MessageTimelineBlock lifecycle", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("shows the timeline waiting indicator while a live turn is between completed tool work and the next server item", async () => {
    const { getByLabelText } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-awaiting-next-item",
            kind: "timeline",
            live: true,
            items: [
              {
                itemId: "tool-1",
                kind: "tool_call",
                phase: "acting",
                status: "completed",
                title: "Read",
                callId: "call-1",
                toolName: "Read",
                arguments: { path: "README.md" },
                output: "ok",
                progress: [],
                startedAtMs: Date.now() - 1200,
                completedAtMs: Date.now() - 200,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(getByLabelText("Waiting for timeline update")).not.toBeNull();
  });

  it("keeps a manually collapsed live timeline closed when ordinary events arrive", async () => {
    const runningBlock = {
      id: "timeline-3",
      kind: "timeline" as const,
      live: true,
      items: [
        {
          itemId: "thinking-1",
          kind: "reasoning" as const,
          phase: "thinking" as const,
          status: "running" as const,
          text: "Still thinking.",
          startedAtMs: 10,
        },
      ],
    };
    const { container, getByText, rerender } = render(
      <MessageList blocks={[runningBlock]} emptyDetail="" emptyTitle="" />,
    );

    await waitFor(() => {
      expect(container.querySelector("details")?.hasAttribute("open")).toBe(true);
    });
    fireEvent.click(getByText(/Working for/));
    expect(container.querySelector("details")?.hasAttribute("open")).toBe(false);

    rerender(
      <MessageList
        blocks={[
          {
            ...runningBlock,
            items: [
              {
                ...runningBlock.items[0],
                status: "completed" as const,
                completedAtMs: 30,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    await waitFor(() => {
      expect(container.querySelector("details")?.hasAttribute("open")).toBe(false);
    });

    rerender(
      <MessageList
        blocks={[
          {
            ...runningBlock,
            items: [
              {
                ...runningBlock.items[0],
                status: "completed" as const,
                completedAtMs: 30,
              },
              {
                itemId: "tool-1",
                kind: "tool_call" as const,
                phase: "acting" as const,
                status: "running" as const,
                callId: "call-1",
                toolName: "Read",
                arguments: { file: "README.md" },
                progress: [],
                startedAtMs: 31,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    await waitFor(() => {
      expect(container.querySelector("details")?.hasAttribute("open")).toBe(false);
    });

    rerender(
      <MessageList
        blocks={[
          {
            ...runningBlock,
            live: false,
            items: [
              {
                ...runningBlock.items[0],
                status: "completed" as const,
                completedAtMs: 30,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    await waitFor(() => {
      expect(container.querySelector("details")?.hasAttribute("open")).toBe(false);
    });

    fireEvent.click(getByText(/Worked for/));

    await waitFor(() => {
      expect(container.querySelector("details")?.hasAttribute("open")).toBe(true);
    });
  });

  it("keeps a waiting user action visible even after the process was manually collapsed", async () => {
    const item = {
      itemId: "tool-1",
      kind: "tool_call" as const,
      phase: "acting" as const,
      status: "running" as const,
      callId: "call-1",
      toolName: "Bash",
      arguments: { cmd: "pnpm test" },
      progress: [],
      startedAtMs: Date.now() - 1000,
    };
    const { container, getByText, rerender } = render(
      <MessageList
        blocks={[{ id: "timeline-action", kind: "timeline", live: true, items: [item] }]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    fireEvent.click(getByText(/Working for/));
    expect(container.querySelector("details")?.hasAttribute("open")).toBe(false);

    rerender(
      <MessageList
        blocks={[
          {
            id: "timeline-action",
            kind: "timeline",
            live: true,
            items: [{ ...item, status: "waitingForInput" }],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    await waitFor(() => {
      expect(container.querySelector("details")?.hasAttribute("open")).toBe(true);
    });
  });
});
