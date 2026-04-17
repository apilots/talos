// @vitest-environment jsdom

import { render, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { MessageList } from "@/widgets/message-list/MessageList";

describe("MessageList", () => {
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
});
