// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { i18n } from "@/shared/i18n/setup";
import { I18nextProvider } from "react-i18next";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AutomationTaskRow } from "./AutomationTaskRow";
import { AutomationTaskControls } from "./AutomationTaskControls";
import { goal, loop } from "./automation.test-support";
import type { AutomationTask } from "./model";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(1_000_000); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
const tasks: AutomationTask[] = [
  { type: "goal", value: goal({ createdAt: 1000 }) },
  { type: "loop", value: loop({ createdAt: 1000, nextDueAt: 1010 }) },
];
const actions = () => ({ onAction: vi.fn().mockResolvedValue(undefined), onEdit: vi.fn() });

it.each(tasks)("counts the first ten seconds for $type, then waits for actual admission", (task) => {
  const handlers = actions();
  const view = render(<AutomationTaskRow {...handlers} task={task} />);
  expect(screen.getByText("Starts in 10s")).toBeVisible();
  expect(screen.getByRole("time")).toHaveAttribute("datetime", new Date(1_010_000).toISOString());
  act(() => vi.advanceTimersByTime(1_000));
  expect(screen.getByText("Starts in 9s")).toBeVisible();
  act(() => vi.advanceTimersByTime(9_000));
  expect(screen.getByText("Waiting")).toBeVisible();
  expect(screen.queryByText("Starts in 0s")).not.toBeInTheDocument();
  expect(handlers.onAction).not.toHaveBeenCalled();
  view.rerender(<AutomationTaskRow {...handlers} task={{ ...task, value: { ...task.value, latestTurnId: "first-work" } } as AutomationTask} />);
  expect(screen.queryByText("Waiting")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Cancel/ })).not.toBeInTheDocument();
});

it.each(tasks)("cancels a waiting $type with exactly one Clear action", (task) => {
  const handlers = actions();
  const view = render(<AutomationTaskRow {...handlers} task={task} />);
  const cancel = screen.getByRole("button", { name: task.type === "goal" ? "Cancel Goal" : "Cancel Loop" });
  expect(cancel.querySelector("svg")).toHaveAttribute("stroke-width", "1.3");
  fireEvent.click(cancel);
  expect(handlers.onAction.mock.calls).toEqual([[task.type === "goal"
    ? { target: "goal", action: { type: "clear", goalId: task.value.id } }
    : { target: "loop", action: { type: "clear", loopId: task.value.id } }]]);
  view.rerender(<AutomationTaskRow {...handlers} task={task} pending />);
  expect(screen.getAllByRole("button").every((button) => button.hasAttribute("disabled"))).toBe(true);
});

it("gives an exact running Loop Turn priority over stale initial scheduling facts", () => {
  render(<AutomationTaskRow {...actions()} task={tasks[1]!} activeTurn={{ turnId: "first-work", startedAt: 998, origin: { type: "loop", loopId: "loop-1", occurrenceId: "o1", configRevision: 3 } }} />);
  expect(screen.getByText("2s")).toBeVisible();
  expect(screen.queryByText(/Starts in/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Cancel Loop" })).not.toBeInTheDocument();
});

it("keeps the Goal grace period while its registration Turn already carries the bound origin", () => {
  render(<AutomationTaskRow {...actions()} task={tasks[0]!} activeTurn={{ turnId: "registration", startedAt: 995, origin: { type: "goal", goalId: "goal-1", objectiveRevision: 2 } }} />);
  expect(screen.getByText("Starts in 10s")).toBeVisible();
  expect(screen.getByRole("button", { name: "Cancel Goal" })).toBeVisible();
});

it("does not invent a wakeup for a null-next Loop without a real queue item", () => {
  const task = { type: "loop" as const, value: loop({ createdAt: 1000, nextDueAt: null }) };
  const handlers = actions();
  const view = render(<AutomationTaskRow {...handlers} task={task} />);
  expect(screen.getByText("Model decides")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Cancel Loop" })).not.toBeInTheDocument();
  view.rerender(<AutomationTaskRow {...handlers} task={task} queuedLoop />);
  expect(screen.getByText("Waiting")).toBeVisible();
  expect(screen.getByRole("button", { name: "Cancel Loop" })).toBeVisible();
});

it("keeps later Loop cadence and paused Goal semantics without restarting the grace period", () => {
  const handlers = actions();
  const view = render(<AutomationTaskRow {...handlers} task={{ type: "loop", value: loop({ createdAt: 1000, nextDueAt: 1010, latestTurnId: "prior-work" }) }} />);
  expect(screen.queryByText(/Starts in|Waiting/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Clear Loop" })).toBeVisible();
  view.rerender(<AutomationTaskRow {...handlers} task={{ type: "goal", value: goal({ status: "paused", createdAt: 1000 }) }} />);
  expect(screen.getByRole("button", { name: "Resume Goal" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Cancel Goal" })).not.toBeInTheDocument();
  act(() => vi.advanceTimersByTime(11_000));
  view.rerender(<AutomationTaskRow {...handlers} task={{ type: "goal", value: goal({ createdAt: 1000 }) }} />);
  expect(screen.getByText("Waiting")).toBeVisible();
  expect(screen.queryByText("Starts in 10s")).not.toBeInTheDocument();
});

it("leaves management controls on the original Clear action unless cancellation is explicitly requested", () => {
  render(<AutomationTaskControls {...actions()} task={tasks[0]!} />);
  expect(screen.getByRole("button", { name: "Clear Goal" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Cancel Goal" })).not.toBeInTheDocument();
});

it("localizes the countdown, wait and cancellation labels", () => {
  const language = i18n.cloneInstance({ lng: "zh-CN", initAsync: false });
  render(<I18nextProvider i18n={language}><AutomationTaskRow {...actions()} task={tasks[0]!} /></I18nextProvider>);
  expect(screen.getByText("10 秒后开始")).toBeVisible();
  expect(screen.getByRole("button", { name: "取消 Goal" })).toBeVisible();
  act(() => vi.advanceTimersByTime(10_000));
  expect(screen.getByText("等待中")).toBeVisible();
});
