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
      expect(container.querySelector('[data-response-surface="plain"]')).not.toBeNull();
      expect(container.querySelector('[data-response-actions="true"]')).not.toBeNull();
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

  it("does not show a copy action on user messages", () => {
    render(
      <MessageList
        blocks={[
          {
            id: "user-1",
            kind: "user",
            text: "What model are you?",
            fileIds: [],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(screen.getByText("What model are you?")).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Copy block" })).toBeNull();
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
});
