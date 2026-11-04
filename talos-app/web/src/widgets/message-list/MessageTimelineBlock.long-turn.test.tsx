// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { MessageTimelineBlock } from "./MessageTimelineBlock";
import type { ChatRenderBlock } from "@/shared/types/ui";
import "@/shared/i18n/setup";

afterEach(cleanup);

it("bounds mounted steps and reaches the earliest step across the final partial page", () => {
  const block: Extract<ChatRenderBlock, { kind: "timeline" }> = {
    id: "paged-turn", kind: "timeline", live: true,
    items: Array.from({ length: 130 }, (_, index) => ({
      kind: "reasoning", phase: "thinking", itemId: String(index), startedAtMs: index,
      status: index === 129 ? "running" : "completed", text: `Step ${index}`,
    })),
  };
  const { container } = render(<MessageTimelineBlock block={block}
    copiedLabel="Copied" copyLabel="Copy" helpfulLabel="Helpful" notHelpfulLabel="Not helpful" />);
  expect(container.querySelectorAll("[data-process-step]")).toHaveLength(64);
  expect(container.querySelector('[data-timeline-item-key="paged-turn:0"]')).toBeNull();
  fireEvent.click(screen.getByRole("button"));
  expect(container.querySelectorAll("[data-process-step]")).toHaveLength(128);
  fireEvent.click(screen.getByRole("button"));
  expect(container.querySelectorAll("[data-process-step]")).toHaveLength(130);
  expect(container.querySelector('[data-timeline-item-key="paged-turn:0"]')).toBeTruthy();
  expect(screen.queryByRole("button")).toBeNull();
});

it("keeps an unanswered question draft when the process is collapsed and reopened", () => {
  const action = {
    actionId: "question", itemId: "question", callId: "question", state: "pending" as const,
    params: { type: "question" as const, params: { header: "Continue", question: "What next?", options: [] } },
  };
  const { container } = render(<MessageTimelineBlock block={{
    id: "question-turn", kind: "timeline", live: true,
    items: [{ itemId: "question", kind: "tool_call", phase: "acting", status: "waitingForInput",
      callId: "question", toolName: "AskQuestion", arguments: null, progress: [], startedAtMs: 0 }],
  }} pendingUserAction={action}
    renderUserAction={() => <textarea aria-label="Answer" defaultValue="" />}
    copiedLabel="Copied" copyLabel="Copy" helpfulLabel="Helpful" notHelpfulLabel="Not helpful" />);
  fireEvent.change(screen.getByRole("textbox", { name: "Answer" }), { target: { value: "Keep my draft" } });
  const summary = container.querySelector("summary")!;
  fireEvent.click(summary);
  fireEvent.click(summary);
  expect((screen.getByRole("textbox", { name: "Answer" }) as HTMLTextAreaElement).value).toBe("Keep my draft");
});
