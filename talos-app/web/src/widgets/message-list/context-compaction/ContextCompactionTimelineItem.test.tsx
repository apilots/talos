// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import "@/shared/i18n/setup";
import { MessageList } from "@/widgets/message-list/MessageList";

describe("ContextCompactionTimelineItem", () => {
  afterEach(cleanup);

  it("renders automatic compaction as a semantic Turn process item", () => {
    render(
      <MessageList
        blocks={[
          {
            id: "timeline-compaction",
            kind: "timeline",
            live: true,
            items: [
              {
                itemId: "compact-1",
                kind: "context_compaction",
                phase: "system",
                status: "running",
                text: "",
                trigger: "threshold_exceeded",
                stage: "summarizing",
                outcome: null,
                metrics: null,
                startedAtMs: Date.now(),
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(screen.getByText("Automatically compacting context")).not.toBeNull();
    expect(
      screen.getByText("Summarizing earlier conversation to free context…"),
    ).not.toBeNull();
    expect(screen.getByText("Summarizing")).not.toBeNull();
  });

  it("uses working-context copy and valid terminal metrics", () => {
    render(
      <MessageList
        blocks={[
          {
            id: "timeline-working-context",
            kind: "timeline",
            items: [
              {
                itemId: "compact-working",
                kind: "context_compaction",
                phase: "system",
                status: "completed",
                text: "",
                trigger: "working_context_exceeded",
                stage: "completed",
                outcome: "applied",
                metrics: {
                  covers_from_seq: null,
                  covers_until_seq: null,
                  pre_compaction_total_tokens: 128_000,
                  post_compaction_total_tokens: 74_000,
                  compaction_input_tokens: 0,
                  compaction_output_tokens: 0,
                  trimmed_context_unit_count: 0,
                  raw_tail_preferred_count: null,
                  raw_tail_backfilled_count: null,
                  working_context_input_messages: 10,
                  working_context_compacted_messages: 6,
                  working_context_preserved_recent_exchanges: 2,
                },
                startedAtMs: 1,
                completedAtMs: 2,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    fireEvent.click(screen.getByText("Worked for 1ms"));
    expect(screen.getByText("Working context reduced")).not.toBeNull();
    expect(screen.getByText("128K → 74K")).not.toBeNull();
    expect(
      screen.getByText("Continuing the turn with recent work preserved."),
    ).not.toBeNull();
  });

  it("keeps user-requested maintenance out of the Turn process rail", () => {
    const { container } = render(
      <MessageList
        blocks={[
          {
            id: "timeline-manual-compaction",
            kind: "timeline",
            items: [
              {
                itemId: "compact-manual",
                kind: "context_compaction",
                phase: "system",
                status: "completed",
                text: "",
                trigger: "user_requested",
                stage: "completed",
                outcome: "applied",
                metrics: null,
                startedAtMs: 1,
                completedAtMs: 2,
              },
            ],
          },
        ]}
        emptyDetail=""
        emptyTitle=""
      />,
    );

    expect(container.querySelector("details")).toBeNull();
    expect(container.firstElementChild?.childElementCount).toBe(0);
  });
});
