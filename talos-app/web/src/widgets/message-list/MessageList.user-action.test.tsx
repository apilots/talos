// @vitest-environment jsdom

import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { MessageList } from "@/widgets/message-list/MessageList";

describe("MessageList user actions", () => {
  afterEach(cleanup);

  it("marks user messages as scroll anchors with their render identity", () => {
    const { container } = render(
      <MessageList
        blocks={[{
          id: "user-message-1",
          kind: "user",
          text: "Keep this prompt stable.",
          fileIds: [],
          live: true,
        }]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(
      container.querySelector('[data-chat-user-block-id="user-message-1"]'),
    ).not.toBeNull();
  });


  it("attaches a pending user action only to its backend timeline item id", () => {
    const { container } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-user-action",
            kind: "timeline",
            live: true,
            items: [
              {
                itemId: "tool-other",
                kind: "tool_call",
                phase: "acting",
                status: "waiting_user_action",
                callId: "call-shared",
                toolName: "Bash",
                arguments: null,
                progress: [],
                startedAtMs: 1,
              },
              {
                itemId: "tool-target",
                kind: "tool_call",
                phase: "acting",
                status: "waiting_user_action",
                callId: "call-shared",
                toolName: "Bash",
                arguments: null,
                progress: [],
                startedAtMs: 2,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
        pendingUserAction={{
          actionId: "action-1",
          itemId: "tool-target",
          callId: "call-shared",
          state: "pending",
          params: {
            type: "dynamicToolResult",
            params: { toolName: "Bash" },
          },
        }}
        renderUserAction={() => <div data-user-action="true">Approval</div>}
      />,
    );

    const other = container.querySelector('[data-timeline-item-key="timeline-user-action:tool-other"]');
    const target = container.querySelector('[data-timeline-item-key="timeline-user-action:tool-target"]');
    expect(other?.querySelector('[data-user-action="true"]')).toBeNull();
    expect(target?.querySelector('[data-user-action="true"]')).not.toBeNull();
  });

});
