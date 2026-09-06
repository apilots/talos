// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { MessageList } from "@/widgets/message-list/MessageList";

describe("MessageTimelineBlock replay", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("moves unknown text from process commentary after the server commits a final answer", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    const processItem = {
      itemId: "thinking-1",
      kind: "reasoning" as const,
      phase: "thinking" as const,
      status: "completed" as const,
      text: "Checked the workspace.",
      startedAtMs: 1,
      completedAtMs: 9,
    };
    const responseItem = {
      itemId: "answer-1",
      sourceItemId: "answer-1",
      kind: "assistant_text" as const,
      phase: "pending" as const,
      status: "running" as const,
      text: "Streaming **answer** renders in visible chunks.",
      startedAtMs: 10,
    };
    const { container, rerender } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-session-1-message-1",
            kind: "timeline",
            live: true,
            items: [processItem, responseItem],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    const unknownItem = container.querySelector('[data-timeline-item-key="timeline-session-1-message-1:answer-1"]');
    expect(unknownItem?.getAttribute("data-timeline-role")).toBe("commentary");

    rerender(
      <MessageList
        blocks={[
          {
            id: "timeline-session-1-message-1",
            kind: "timeline",
            live: true,
            items: [
              processItem,
              { ...responseItem, phase: "final_answer", replayFinalAnswer: true, status: "running" },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    const committedResponse = container.querySelector('[data-timeline-item-key="timeline-session-1-message-1:answer-1"]');
    expect(committedResponse?.getAttribute("data-timeline-response")).toBe("true");
    expect(screen.queryByText("Final answer")).toBeNull();
    expect(container.querySelector(".markdown-body")).toBeNull();
    await waitFor(() => {
      expect(container.querySelector(".markdown-body strong")?.textContent).toBe("answer");
    });

    rerender(
      <MessageList
        blocks={[
          {
            id: "timeline-session-1-message-1",
            kind: "timeline",
            live: false,
            items: [
              processItem,
              { ...responseItem, phase: "final_answer", status: "completed" },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(container.querySelector('[data-timeline-item-key="timeline-session-1-message-1:answer-1"]')).toBe(committedResponse);
    expect(screen.queryByText("Final answer")).toBeNull();
    await waitFor(() => {
      expect(container.querySelector(".markdown-body strong")?.textContent).toBe("answer");
    });

    const completedResponse = container.querySelector('[data-timeline-response="true"]');
    const responseCopyButton = completedResponse?.parentElement?.querySelector<HTMLButtonElement>(
      'button[aria-label="Copy block"]',
    );
    expect(responseCopyButton).toBeInstanceOf(HTMLButtonElement);
    fireEvent.click(responseCopyButton as HTMLButtonElement);

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("Streaming **answer** renders in visible chunks.");
    });
  });

  it("presents unknown assistant text followed by tool work as commentary", () => {
    const { container } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-commentary-before-tool",
            kind: "timeline",
            live: true,
            items: [
              {
                itemId: "commentary-1",
                kind: "assistant_text",
                phase: "pending",
                status: "completed",
                text: "I will inspect the implementation.",
                startedAtMs: 1,
                completedAtMs: 2,
              },
              {
                itemId: "tool-1",
                kind: "tool_call",
                phase: "acting",
                status: "running",
                callId: "call-1",
                toolName: "Read",
                arguments: { path: "src/main.ts" },
                progress: [],
                startedAtMs: 3,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(container.querySelector('[data-timeline-role="commentary"]')?.textContent).toContain(
      "I will inspect the implementation.",
    );
  });
});
