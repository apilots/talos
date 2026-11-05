// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AutomationTaskRow } from "./AutomationTaskRow";
import { goal, loop } from "./automation.test-support";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(1_000_000); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
const actions = { onAction: vi.fn(), onEdit: vi.fn() };
it("counts the whole Goal across pause, resume and edits, then freezes at its completion", () => {
  const task = { type: "goal" as const, value: goal({ createdAt: 900, latestTurnId: "first-work" }) };
  const view = render(<AutomationTaskRow {...actions} task={task} />);
  expect(screen.getByText("1m40s")).toBeInTheDocument();
  act(() => vi.advanceTimersByTime(2_000));
  expect(screen.getByText("1m42s")).toBeInTheDocument();
  view.rerender(<AutomationTaskRow {...actions} task={{ ...task, value: { ...task.value, status: "paused" } }} />);
  expect(screen.getByRole("button", { name: "Resume Goal" }).querySelector("circle")).toHaveAttribute("r", "7");
  expect(screen.getByRole("button", { name: "Resume Goal" }).querySelector("path")).toHaveAttribute("d", "m8 6 5 4-5 4z");
  act(() => vi.advanceTimersByTime(2_000));
  expect(screen.getByText("1m44s")).toBeInTheDocument();
  view.rerender(<AutomationTaskRow {...actions} task={{ ...task, value: { ...task.value, objective: "Edited objective", latestTurnId: "next-turn", status: "active" } }} />);
  expect(screen.getByText("1m44s")).toBeInTheDocument();
  view.rerender(<AutomationTaskRow {...actions} task={{ ...task, value: { ...task.value, status: "complete", updatedAt: 1004 } }} />);
  act(() => vi.advanceTimersByTime(60_000));
  expect(screen.getByText("1m44s")).toBeInTheDocument();
  expect(vi.getTimerCount()).toBe(0);
});
it("resets a Loop timer for each exact live Turn, then shows the absolute next time", () => {
  const task = { type: "loop" as const, value: loop({ nextDueAt: 2000, latestTurnId: "first-work" }) };
  const origin = { type: "loop" as const, loopId: task.value.id, occurrenceId: "o1", configRevision: 3 };
  const view = render(<AutomationTaskRow {...actions} task={task} activeTurn={{ turnId: "t1", origin, startedAt: 990 }} />);
  expect(screen.getByText("Loop")).toBeVisible();
  expect(screen.getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual(["Stop Loop", "Clear Loop", "Edit Loop"]);
  expect(screen.getByText("10s")).toBeInTheDocument();
  act(() => vi.advanceTimersByTime(2_000));
  expect(screen.getByText("12s")).toBeInTheDocument();
  view.rerender(<AutomationTaskRow {...actions} task={task} activeTurn={{ turnId: "t2", origin: { ...origin, occurrenceId: "o2" }, startedAt: 1002 }} />);
  expect(screen.getByText("0s")).toBeInTheDocument();
  act(() => vi.advanceTimersByTime(1_000));
  expect(screen.getByText("1s")).toBeInTheDocument();
  view.rerender(<AutomationTaskRow {...actions} task={task} />);
  expect(screen.getByRole("time")).toHaveAttribute("datetime", new Date(2_000_000).toISOString());
  expect(screen.queryByText("1s")).not.toBeInTheDocument();
  expect(vi.getTimerCount()).toBe(0);
});
it("does not invent running time from an active Loop definition or another Loop's Turn", () => {
  const task = { type: "loop" as const, value: loop({ nextDueAt: null }) };
  const view = render(<AutomationTaskRow {...actions} task={task} activeTurn={{ turnId: "t1", origin: { type: "loop", loopId: "another-loop", occurrenceId: "o1", configRevision: 3 }, startedAt: 990 }} />);
  expect(screen.getByText("Model decides")).toBeInTheDocument();
  expect(screen.queryByText("10s")).not.toBeInTheDocument();
  view.rerender(<AutomationTaskRow {...actions} task={{ ...task, value: { ...task.value, schedule: { type: "fixedInterval", minutes: 5 } } }} />);
  expect(screen.getByText("No next time yet")).toBeInTheDocument();
  view.rerender(<AutomationTaskRow {...actions} task={{ ...task, value: { ...task.value, status: "stopped", nextDueAt: null } }} />);
  expect(screen.getByText("Stopped")).toBeInTheDocument();
});
it("keeps the requested compact action order and opens the existing editor", () => {
  const onEdit = vi.fn();
  const task = { type: "goal" as const, value: goal({ createdAt: 900, latestTurnId: "first-work" }) };
  render(<AutomationTaskRow {...actions} task={task} onEdit={onEdit} />);
  expect(screen.getByText("Goal")).toBeVisible();
  expect(screen.getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual(["Pause Goal", "Clear Goal", "Edit Goal"]);
  fireEvent.click(screen.getByRole("button", { name: "Edit Goal" }));
  expect(onEdit).toHaveBeenCalledWith(task);
  expect(screen.getByRole("button", { name: "Edit Goal" }).querySelector("path")).toHaveAttribute("d", "M5 8V5h3m4 0h3v3M5 12v3h3m4 0h3v-3");
});
it("keeps the complete absolute next time while splitting its narrow presentation inside the same row", () => {
  const nextDueAt = 2_000;
  render(<AutomationTaskRow {...actions} task={{ type: "loop", value: loop({ nextDueAt, latestTurnId: "first-work" }) }} />);
  const time = screen.getByRole("time");
  expect(time).toHaveAttribute("datetime", new Date(nextDueAt * 1000).toISOString());
  expect(time.getAttribute("title")).toContain("1970");
  const compact = screen.getByTestId("automation-next-time-compact");
  expect(Array.from(compact.children, (part) => part.textContent)).toEqual([
    new Date(nextDueAt * 1000).toLocaleDateString("en", { month: "numeric", day: "numeric" }),
    new Date(nextDueAt * 1000).toLocaleTimeString("en", { hour: "2-digit", minute: "2-digit", hour12: false }),
  ]);
  expect(compact).toHaveClass("sm:hidden");
  expect(time.querySelector(".sm\\:inline")).toHaveTextContent(new Date(nextDueAt * 1000).toLocaleString("en", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }));
  expect(screen.getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual(["Stop Loop", "Clear Loop", "Edit Loop"]);
});
