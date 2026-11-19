// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { MessageTimelineBlock } from "./MessageTimelineBlock";
import type { ChatRenderBlock, ChatTimelineItem } from "@/shared/types/ui";
import "@/shared/i18n/setup";

afterEach(cleanup);

it("bounds mounted steps and reaches the earliest step across the final partial page", async () => {
  const block: Extract<ChatRenderBlock, { kind: "timeline" }> = {
    id: "paged-turn", kind: "timeline", live: true,
    items: Array.from({ length: 130 }, (_, index) => ({
      kind: "reasoning", phase: "thinking", itemId: String(index), startedAtMs: index,
      status: index === 129 ? "running" : "completed", text: `Step ${index}`,
    })),
  };
  const { container } = render(<MessageTimelineBlock block={block}
    copiedLabel="Copied" copyLabel="Copy" helpfulLabel="Helpful" notHelpfulLabel="Not helpful" />);
  expect(container.querySelectorAll("[data-process-step]")).toHaveLength(0);
  fireEvent.click(screen.getByText("Thinking"));
  await waitFor(() => expect(container.querySelectorAll("[data-process-step]")).toHaveLength(64));
  expect(container.querySelector('[data-timeline-item-key="paged-turn:0"]')).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Show 64 earlier steps" }));
  expect(container.querySelectorAll("[data-process-step]")).toHaveLength(128);
  fireEvent.click(screen.getByRole("button", { name: "Show 2 earlier steps" }));
  expect(container.querySelectorAll("[data-process-step]")).toHaveLength(130);
  expect(container.querySelector('[data-timeline-item-key="paged-turn:0"]')).toBeTruthy();
  expect(screen.queryByRole("button", { name: /earlier steps/ })).toBeNull();
});

it("keeps an unanswered question visible and preserves its draft when activity is toggled", async () => {
  const action = {
    actionId: "question", itemId: "question", callId: "question", state: "pending" as const,
    params: { type: "question" as const, params: { header: "Continue", question: "What next?", options: [] } },
  };
  render(<MessageTimelineBlock block={{
    id: "question-turn", kind: "timeline", live: true,
    items: [
      { itemId: "reasoning", kind: "reasoning", phase: "thinking", status: "completed", text: "Check with the user.", startedAtMs: 0 },
      { itemId: "question", kind: "tool_call", phase: "acting", status: "waitingForInput",
        callId: "question", toolName: "AskQuestion", arguments: null, progress: [], startedAtMs: 0 },
    ],
  }} pendingUserAction={action}
    renderUserAction={() => <textarea aria-label="Answer" defaultValue="" />}
    copiedLabel="Copied" copyLabel="Copy" helpfulLabel="Helpful" notHelpfulLabel="Not helpful" />);
  const answer = screen.getByRole("textbox", { name: "Answer" }) as HTMLTextAreaElement;
  expect(answer.closest("details")).toBeNull();
  expect(screen.getByText("Waiting for input")).toBeTruthy();
  expect(screen.queryByRole("status")).toBeNull();
  fireEvent.change(answer, { target: { value: "Keep my draft" } });
  fireEvent.click(screen.getByText("Thinking"));
  await screen.findByText("Check with the user.");
  fireEvent.click(screen.getByRole("button", { name: /^Working/ }));
  expect(screen.queryByText("Thinking")).toBeNull();
  expect(screen.getByRole("textbox", { name: "Answer" })).toBe(answer);
  expect(answer.value).toBe("Keep my draft");
});

it("pages interleaved history while keeping the latest commentary and active command accessible", () => {
  const history = Array.from({ length: 80 }, (_, index): ChatTimelineItem[] => [
    { itemId: `thinking-${index}`, kind: "reasoning", phase: "thinking", status: "completed",
      text: `Thinking ${index}`, startedAtMs: 10 + index * 20 },
    { itemId: `tool-${index}`, kind: "tool_call", phase: "acting", status: "completed", callId: `call-${index}`,
      toolName: "Bash", arguments: { command: `echo ordered-${index}` }, progress: [], startedAtMs: 20 + index * 20 },
  ]).flat();
  const { container } = render(<MessageTimelineBlock block={{
    id: "interleaved", kind: "timeline", live: true, items: [
      { itemId: "latest", kind: "assistant_text", phase: "commentary", status: "completed",
        text: "The batch is being checked.", startedAtMs: 1 },
      { itemId: "ongoing", kind: "tool_call", phase: "acting", status: "running", callId: "ongoing",
        toolName: "Bash", arguments: { command: "sleep 120" }, progress: [], startedAtMs: 2 },
      ...history,
    ],
  }} copiedLabel="Copied" copyLabel="Copy" helpfulLabel="Helpful" notHelpfulLabel="Not helpful" />);
  expect(container.querySelectorAll("[data-timeline-activity], [data-timeline-role='tool']")).toHaveLength(65);
  expect(screen.getByText("The batch is being checked.")).toBeTruthy();
  expect(screen.getByText("No command output yet")).toBeTruthy();
  expect(screen.queryByText("Bash · echo ordered-0")).toBeNull();
  expect(screen.getByText("Bash · echo ordered-79")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Show earlier steps" }));
  expect(container.querySelectorAll("[data-timeline-activity], [data-timeline-role='tool']")).toHaveLength(129);
  fireEvent.click(screen.getByRole("button", { name: "Show earlier steps" }));
  expect(screen.getByText("Bash · echo ordered-0")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Show earlier steps" })).toBeNull();
});
