// @vitest-environment jsdom

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { MessageList } from "@/widgets/message-list/MessageList";

describe("MessageList timeline status", () => {
  it("renders queued tool calls without marking them as running", () => {
    const { queryByText } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-queued-tool",
            kind: "timeline",
            live: true,
            items: [
              {
                itemId: "tool-queued",
                kind: "tool_call",
                phase: "acting",
                status: "queued",
                callId: "call-queued",
                toolName: "Read",
                arguments: { file: "README.md" },
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

    expect(screen.getByText("Queued")).not.toBeNull();
    expect(queryByText("Running")).toBeNull();
  });
});
