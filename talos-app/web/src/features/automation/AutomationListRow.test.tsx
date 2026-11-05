// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { i18n } from "@/shared/i18n/setup";
import { I18nextProvider } from "react-i18next";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AutomationListRow } from "./AutomationListRow";
import { goal, loop } from "./automation.test-support";
import type { AutomationTask } from "./model";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(1_000_000); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
function setup(task: AutomationTask, language = "en", pending = false) {
  const onAction = vi.fn().mockResolvedValue(undefined);
  const onEdit = vi.fn();
  const locale = i18n.cloneInstance({ lng: language, initAsync: false });
  render(<I18nextProvider i18n={locale}><MemoryRouter><AutomationListRow task={task} pending={pending} onAction={onAction} onEdit={onEdit} /></MemoryRouter></I18nextProvider>);
  return { onAction, onEdit, row: screen.getByTestId(`automation-list-task-${task.value.id}`) };
}

describe.each(["en", "zh-CN"])("quiet management rows (%s)", (language) => {
  it("shows a completed Goal once with elapsed metadata, without token counts or technical chat text", () => {
    const task = { type: "goal" as const, value: goal({ status: "complete", sessionId: "technical-session-id", createdAt: 900, updatedAt: 1004, tokensUsed: 987654, tokenBudget: 1234567 }) };
    const { row } = setup(task, language);
    expect(within(row).getByRole("heading", { name: "Fix CI" })).toHaveClass("truncate");
    expect(within(row).getAllByText(language === "en" ? "Complete" : "已完成")).toHaveLength(1);
    expect(within(row).getByTestId("automation-list-metadata")).toContainElement(within(row).getByTestId("automation-list-status"));
    expect(within(row).getByTestId("automation-list-metadata")).toHaveTextContent(language === "en" ? "Goal · Elapsed 1m44s" : "Goal · 耗时 1m44s");
    expect(row.querySelectorAll("p")).toHaveLength(1);
    expect(row.textContent).not.toMatch(/987654|1234567|technical-session-id/);
    const link = within(row).getByRole("link", { name: language === "en" ? "Open chat: Fix CI" : "回到会话: Fix CI" });
    expect(link).toHaveAttribute("href", "/sessions/technical-session-id");
    expect(link).toHaveAttribute("title", language === "en" ? "Open chat" : "回到会话");
    expect(link.textContent).toBe("");
    act(() => vi.advanceTimersByTime(10_000));
    expect(within(row).getByTestId("automation-list-metadata")).toHaveTextContent("1m44s");
  });
  it("keeps a stopped Loop's reason in one metadata line and its state only once", () => {
    const task = { type: "loop" as const, value: loop({ status: "stopped", stopReason: "modelStopped", reason: "Deployment is healthy; no more checks needed" }) };
    const { row } = setup(task, language);
    expect(within(row).getAllByText(language === "en" ? "Stopped" : "已停止")).toHaveLength(1);
    const metadata = within(row).getByTestId("automation-list-metadata");
    expect(metadata).toHaveClass("truncate");
    expect(metadata).toHaveTextContent(language === "en" ? "Loop · Model decides · Stopped · Deployment is healthy" : "Loop · 模型决定 · 已停止 · Deployment is healthy");
    expect(within(row).getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual([language === "en" ? "Clear Loop" : "清除 Loop"]);
    expect(row.querySelectorAll("p")).toHaveLength(1);
  });
});

it("reuses the original typed controls and keeps edit focus for the existing modal", () => {
  const task = { type: "goal" as const, value: goal() };
  const { onAction, onEdit, row } = setup(task);
  const buttons = within(row).getAllByRole("button");
  expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual(["Pause Goal", "Clear Goal", "Edit Goal"]);
  fireEvent.click(buttons[0]!);
  expect(onAction).toHaveBeenCalledWith({ target: "goal", action: { type: "pause", goalId: task.value.id } });
  fireEvent.click(buttons[2]!);
  expect(onEdit).toHaveBeenCalledWith(task);
  expect(document.activeElement).toBe(buttons[2]);
});
it("keeps pending controls disabled and has a bounded two-line layout for narrow screens", () => {
  const task = { type: "loop" as const, value: loop({ prompt: "A long deployment follow-up that needs to remain readable in a 360px viewport", nextDueAt: 2000 }) };
  const { onAction, row } = setup(task, "en", true);
  expect(row).toHaveClass("grid-cols-[minmax(0,1fr)_auto]", "min-w-0");
  expect(within(row).getByRole("heading")).toHaveClass("truncate");
  expect(within(row).getByTestId("automation-list-actions")).toHaveClass("col-start-2", "row-start-2", "shrink-0");
  expect(within(row).getByTestId("automation-list-metadata")).toHaveClass("truncate");
  expect(within(row).getByTestId("automation-list-metadata")).toHaveTextContent("Next");
  const buttons = within(row).getAllByRole("button");
  expect(buttons.every((button) => button.hasAttribute("disabled"))).toBe(true);
  fireEvent.click(buttons[0]!);
  expect(onAction).not.toHaveBeenCalled();
  expect(within(row).getByRole("link")).toBeInTheDocument();
});
it("uses a short translated budget reason and keeps pause included in the displayed Goal time", () => {
  const task = { type: "goal" as const, value: goal({ status: "paused", pauseReason: "budgetExhausted", createdAt: 900 }) };
  setup(task, "zh-CN");
  const metadata = screen.getByTestId("automation-list-metadata");
  expect(metadata).toHaveTextContent("Goal · 耗时 1m40s · 已暂停 · 已达预算");
  act(() => vi.advanceTimersByTime(2_000));
  expect(metadata).toHaveTextContent("1m42s");
  expect(metadata).not.toHaveTextContent("2000");
});
