// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { MessageList } from "@/widgets/message-list/MessageList";

describe("MessageList", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("renders markdown structure for assistant messages", async () => {
    const { container } = render(
      <MessageList
        blocks={[
          {
            id: "assistant-1",
            kind: "assistant",
            text: "# Title\n\nA paragraph with `inline code`.\n\n- First\n- Second\n\n```ts\nconst value = 1;\n```",
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    await waitFor(() => {
      expect(container.querySelector(".markdown-body h1")?.textContent).toBe("Title");
      expect(container.querySelector(".markdown-body ul")).not.toBeNull();
      expect(container.querySelector(".markdown-body pre code")?.textContent).toContain("const value = 1;");
    });
  });

  it("keeps inline code and list items readable in continuity sections", async () => {
    const { container } = render(
      <MessageList
        blocks={[
          {
            id: "assistant-2",
            kind: "assistant",
            text:
              "## Session Continuity（跨 Turn 连续性）\n\n在 `prepare.rs` 中实现，保存上次 turn 的执行痕迹：\n- `recent_verified_paths`：最近验证过的路径\n- `recent_successful_tools` / `recent_failed_tools`\n- `recent_unhelpful_patterns`：避免重复无效操作",
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    await waitFor(() => {
      const heading = container.querySelector(".markdown-body h2");
      const items = container.querySelectorAll(".markdown-body li");
      const inlineCode = container.querySelectorAll(".markdown-body li code");

      expect(heading?.textContent).toContain("Session Continuity");
      expect(items).toHaveLength(3);
      expect(items[0]?.textContent).toContain("recent_verified_paths");
      expect(inlineCode[0]?.textContent).toBe("recent_verified_paths");
    });
  });

  it("renders animated status placeholders inside the transcript", async () => {
    const { getByText } = render(
      <MessageList
        blocks={[
          {
            id: "status-1",
            kind: "status",
            variant: "thinking",
            title: "Waiting for server response",
            detail: "Talos is still preparing the first reply chunk.",
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    await waitFor(() => {
      expect(getByText("Waiting for server response")).not.toBeNull();
      expect(getByText("Talos is still preparing the first reply chunk.")).not.toBeNull();
    });
  });

  it("copies the original markdown for text blocks", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(
      <MessageList
        blocks={[
          {
            id: "assistant-live",
            kind: "assistant",
            text: "# Partial\n\nFull **markdown** body.",
            live: true,
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy block" }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("# Partial\n\nFull **markdown** body.");
    });
  });

  it("copies tool blocks as markdown with arguments, progress, and output", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    render(
      <MessageList
        blocks={[
          {
            id: "tool-1",
            kind: "tool",
            callId: "call-1",
            toolName: "Bash",
            arguments: { cmd: "pnpm test" },
            status: "done",
            progress: [{ kind: "stdout", message: "running" }],
            output: "ok",
            timingMs: 42,
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Copy block" }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining("### Tool: Bash"));
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining('"cmd": "pnpm test"'));
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining("- **stdout**: running"));
      expect(writeText).toHaveBeenCalledWith(expect.stringContaining("ok"));
    });
  });

  it("renders timeline blocks with product labels, tool progress, and duration", async () => {
    const { getAllByText, getByText, queryByText } = render(
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
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(getByText("Thinking / Acting")).not.toBeNull();
    expect(getByText("Completed")).not.toBeNull();
    expect(getByText("46 tokens")).not.toBeNull();
    expect(getByText((content) => content === "315ms")).not.toBeNull();
    fireEvent.click(getByText("Thinking / Acting"));

    await waitFor(() => {
      expect(getByText("Checking context")).not.toBeNull();
      expect(getByText("I will inspect the workspace.")).not.toBeNull();
      expect(getAllByText("Thinking").length).toBeGreaterThanOrEqual(2);
      expect(getAllByText("Bash").length).toBeGreaterThan(0);
      expect(getByText("300ms")).not.toBeNull();
      expect(queryByText("completed · 300ms")).toBeNull();
      expect(queryByText("assistant text")).toBeNull();
      expect(queryByText("commentary")).toBeNull();
    });
  });

  it("renders thinking text as ordered timeline thought lines", async () => {
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
                text: "First thought. Second thought.",
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

    expect(getByText("First thought.")).not.toBeNull();
    expect(getByText("Second thought.")).not.toBeNull();
    expect(getAllByText("Glob").length).toBeGreaterThan(0);
    expect(getByLabelText("Waiting for timeline update")).not.toBeNull();
    expect(container.querySelectorAll("ol > li")).toHaveLength(3);
  });

  it("does not show a running status label on thinking items", async () => {
    const { getByLabelText, queryByText } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-running-thinking",
            kind: "timeline",
            items: [
              {
                itemId: "thinking-1",
                kind: "reasoning",
                phase: "thinking",
                status: "running",
                text: "Checking the current context.",
                startedAtMs: Date.now() - 1200,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(queryByText("running")).toBeNull();
    expect(getByLabelText("Waiting for timeline update")).not.toBeNull();
  });

  it("hides the timeline waiting indicator after the final answer phase starts", async () => {
    const { queryByLabelText } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-final-answer",
            kind: "timeline",
            items: [
              {
                itemId: "answer-1",
                kind: "assistant_text",
                phase: "final_answer",
                status: "running",
                text: "Streaming final answer.",
                startedAtMs: Date.now() - 1200,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(queryByLabelText("Waiting for timeline update")).toBeNull();
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

  it("keeps live timeline open across idle gaps and reopens after manual collapse when new events arrive", async () => {
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
    fireEvent.click(getByText("Thinking / Acting"));
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
      expect(container.querySelector("details")?.hasAttribute("open")).toBe(true);
    });

    fireEvent.click(getByText("Thinking / Acting"));
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
      expect(container.querySelector("details")?.hasAttribute("open")).toBe(true);
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

    fireEvent.click(getByText("Thinking / Acting"));

    await waitFor(() => {
      expect(container.querySelector("details")?.hasAttribute("open")).toBe(true);
    });
  });
});
