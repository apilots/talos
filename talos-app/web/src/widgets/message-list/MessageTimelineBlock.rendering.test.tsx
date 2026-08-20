// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { MessageList } from "@/widgets/message-list/MessageList";

describe("MessageTimelineBlock rendering", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("renders a compact process timeline with distinct thinking and commentary roles", async () => {
    const { container, getAllByText, getByText } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-1",
            kind: "timeline",
            metrics: {
              state: "completed",
              model_rounds: 2,
              tool_calls: 1,
              tool_successes: 1,
              tool_errors: 0,
              stop_reason: "end_turn",
              usage: {
                input_tokens: 12,
                output_tokens: 34,
                total_tokens: 46,
                cache_creation_input_tokens: null,
                cache_read_input_tokens: null,
              },
              duration_ms: null,
            },
            items: [
              {
                itemId: "thinking-1",
                kind: "reasoning",
                phase: "thinking",
                status: "completed",
                text: "Checking context",
                startedAtMs: 10,
                completedAtMs: 20,
              },
              {
                itemId: "commentary-1",
                kind: "assistant_text",
                phase: "commentary",
                status: "completed",
                text: "I will inspect the workspace.",
                startedAtMs: 21,
                completedAtMs: 24,
              },
              {
                itemId: "tool-1",
                kind: "tool_call",
                phase: "acting",
                status: "completed",
                callId: "call-1",
                toolName: "Bash",
                arguments: { cmd: "pnpm test" },
                output: "ok",
                progress: [{ kind: "stdout", message: "running" }],
                startedAtMs: 25,
                completedAtMs: 325,
              },
              {
                itemId: "summary-1",
                kind: "execution_summary",
                phase: "system",
                status: "completed",
                text: "Validation completed.",
                startedAtMs: 326,
                completedAtMs: 330,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(getByText("Worked for 320ms")).not.toBeNull();
    expect(screen.queryByText("4 events")).toBeNull();
    expect(screen.queryByText("46 tokens")).toBeNull();
    expect(container.querySelector('[data-process-surface="rail"]')).not.toBeNull();
    expect(container.querySelector("details")?.hasAttribute("open")).toBe(false);
    expect(screen.queryByRole("button", { name: "Copy block" })).toBeNull();
    fireEvent.click(getByText("Worked for 320ms"));

    await waitFor(() => {
      expect(getByText("Checking context")).not.toBeNull();
      expect(getByText("I will inspect the workspace.")).not.toBeNull();
      expect(getByText("Execution summary")).not.toBeNull();
      expect(getByText("Validation completed.")).not.toBeNull();
      expect(getAllByText(/Bash/)).toHaveLength(1);
      expect(getByText(/300ms/)).not.toBeNull();
      expect(container.querySelectorAll('[data-tool-call-card="true"]')).toHaveLength(1);
      expect(container.querySelector('[data-timeline-role="thinking"]')).not.toBeNull();
      expect(container.querySelector('[data-timeline-role="commentary"]')).not.toBeNull();
      expect(container.querySelector('[data-timeline-surface="plain"]')).not.toBeNull();
      expect(container.querySelector('[data-tool-call-variant="compact"]')).not.toBeNull();
      expect(container.querySelector('[data-tool-surface="plain"]')).not.toBeNull();
      expect(container.querySelectorAll('[data-process-step="true"]')).toHaveLength(4);
    });
  });

  it("renders turn metrics once below the final response and outside the process summary", () => {
    const { container } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-metrics",
            kind: "timeline",
            metrics: {
              state: "completed",
              model_rounds: 11,
              tool_calls: 16,
              tool_successes: 16,
              tool_errors: 0,
              stop_reason: "end_turn",
              usage: {
                input_tokens: 900_000,
                output_tokens: 78_200,
                total_tokens: 978_200,
                cache_creation_input_tokens: null,
                cache_read_input_tokens: null,
              },
              duration_ms: 30_000,
            },
            items: [
              {
                itemId: "thinking-1",
                kind: "reasoning",
                phase: "thinking",
                status: "completed",
                text: "Checked the implementation.",
                startedAtMs: 1,
                completedAtMs: 2,
              },
              {
                itemId: "answer-1",
                kind: "assistant_text",
                phase: "final_answer",
                status: "completed",
                text: "The implementation is complete.",
                startedAtMs: 3,
                completedAtMs: 4,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    const footers = container.querySelectorAll('[data-turn-metrics="true"]');
    expect(footers).toHaveLength(1);
    expect(container.querySelector("summary")?.contains(footers[0] as Node)).toBe(false);
    expect(container.querySelector('[data-timeline-response="true"]')?.contains(footers[0] as Node)).toBe(true);
  });

  it("labels a cancelled turn and its tool as stopped without metrics", () => {
    render(
      <MessageList
        blocks={[
          {
            id: "timeline-cancelled",
            kind: "timeline",
            items: [
              {
                itemId: "tool-cancelled",
                kind: "tool_call",
                phase: "acting",
                status: "cancelled",
                callId: "call-cancelled",
                toolName: "Bash",
                arguments: { cmd: "sleep 30" },
                progress: [],
                startedAtMs: 1_000,
                completedAtMs: 14_000,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(screen.getByText("Stopped after 13s")).not.toBeNull();
    expect(screen.queryByText("Worked for 13s")).toBeNull();
    fireEvent.click(screen.getByText("Stopped after 13s"));
    expect(screen.getAllByText("Cancelled").length).toBeGreaterThan(0);
  });

  it("labels a failed turn from its terminal item when metrics are unavailable", () => {
    render(
      <MessageList
        blocks={[
          {
            id: "timeline-failed",
            kind: "timeline",
            items: [
              {
                itemId: "tool-failed",
                kind: "tool_call",
                phase: "acting",
                status: "failed",
                callId: "call-failed",
                toolName: "Bash",
                arguments: { cmd: "exit 1" },
                progress: [],
                startedAtMs: 2_000,
                completedAtMs: 4_000,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(screen.getByText("Failed after 2s")).not.toBeNull();
    expect(screen.queryByText("Worked for 2s")).toBeNull();
  });

  it("preserves complete thinking sentences and model-provided line breaks", async () => {
    const { container, getAllByText, getByLabelText, getByText } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-2",
            kind: "timeline",
            items: [
              {
                itemId: "thinking-1",
                kind: "reasoning",
                phase: "thinking",
                status: "running",
                text: "First thought. Second thought.\nThird thought.",
                startedAtMs: 10,
              },
              {
                itemId: "tool-1",
                kind: "tool_call",
                phase: "acting",
                status: "running",
                callId: "call-1",
                toolName: "Glob",
                arguments: { pattern: "**/*.rs" },
                progress: [],
                startedAtMs: 20,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    const thinkingText = getByText((_, element) =>
      element?.getAttribute("data-timeline-text") === "true",
    );
    expect(thinkingText.textContent).toBe("First thought. Second thought.\nThird thought.");
    expect(getAllByText(/Glob/).length).toBeGreaterThan(0);
    expect(getByLabelText("Waiting for timeline update")).not.toBeNull();
    expect(container.querySelectorAll("ol > li")).toHaveLength(3);
  });
});
