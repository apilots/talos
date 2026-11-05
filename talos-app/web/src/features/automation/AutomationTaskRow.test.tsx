// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AutomationTaskRow } from "./AutomationTaskRow";
import { goal, loop } from "./automation.test-support";
afterEach(cleanup);

it("matches the approved thin Goal and Loop symbols without adding status text", () => {
  const view = render(<AutomationTaskRow task={{ type: "goal", value: goal() }} onAction={vi.fn()} onEdit={vi.fn()} />);
  const icon = screen.getByTestId("automation-task-goal-1").querySelector("svg")!;
  expect(icon).toHaveAttribute("viewBox", "0 0 20 20");
  expect(icon).toHaveAttribute("width", "15");
  expect(icon).toHaveAttribute("stroke-width", "1.3");
  expect(Array.from(icon.querySelectorAll("circle"), (circle) => circle.getAttribute("r"))).toEqual(["6.5", "3"]);
  expect(icon.querySelector("path")).toHaveAttribute("d", "m10 10 6-6m-2 0h2v2");
  expect(screen.queryByText("Active")).not.toBeInTheDocument();
  view.rerender(<AutomationTaskRow task={{ type: "loop", value: loop() }} onAction={vi.fn()} onEdit={vi.fn()} />);
  expect(screen.getByTestId("automation-task-loop-1").querySelector("svg path")).toHaveAttribute("d", "M16 7a6 6 0 0 0-10-2L3 8m0-4v4h4M4 13a6 6 0 0 0 10 2l3-3m0 4v-4h-4");
});

it("keeps compact controls in pause, delete, expand order with the same typed actions", () => {
  const onAction = vi.fn().mockResolvedValue(undefined);
  const onEdit = vi.fn();
  const task = { type: "goal" as const, value: goal({ latestTurnId: "first-work" }) };
  render(<AutomationTaskRow task={task} onAction={onAction} onEdit={onEdit} />);
  const controls = screen.getAllByRole("button");
  expect(controls.map((button) => button.getAttribute("aria-label"))).toEqual(["Pause Goal", "Clear Goal", "Edit Goal"]);
  expect(controls.every((button) => !button.className.includes("hover:bg"))).toBe(true);
  expect(controls[0]!.querySelector("path")).toHaveAttribute("d", "M7 5v10M13 5v10");
  expect(controls[1]!.querySelector("svg")).toHaveAttribute("width", "14");
  fireEvent.click(controls[0]!); fireEvent.click(controls[1]!); fireEvent.click(controls[2]!);
  expect(onAction.mock.calls.map(([action]) => action)).toEqual([
    { target: "goal", action: { type: "pause", goalId: task.value.id } },
    { target: "goal", action: { type: "clear", goalId: task.value.id } },
  ]);
  expect(onEdit).toHaveBeenCalledWith(task);
});

it("retains Stop semantics, pending guards and the stopped Loop's Clear action", () => {
  const onAction = vi.fn().mockResolvedValue(undefined);
  const onEdit = vi.fn();
  const task = { type: "loop" as const, value: loop() };
  const view = render(<AutomationTaskRow task={task} onAction={onAction} onEdit={onEdit} />);
  expect(screen.getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual(["Stop Loop", "Clear Loop", "Edit Loop"]);
  expect(screen.getByRole("button", { name: "Stop Loop" }).querySelector("rect")).toHaveAttribute("width", "10");
  fireEvent.click(screen.getByRole("button", { name: "Stop Loop" }));
  expect(onAction).toHaveBeenCalledWith({ target: "loop", action: { type: "stop", loopId: task.value.id } });
  view.rerender(<AutomationTaskRow task={task} pending onAction={onAction} onEdit={onEdit} />);
  expect(screen.getAllByRole("button").every((button) => button.hasAttribute("disabled"))).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Edit Loop" }));
  expect(onEdit).not.toHaveBeenCalled();
  view.rerender(<AutomationTaskRow task={{ ...task, value: { ...task.value, status: "stopped" } }} onAction={onAction} onEdit={onEdit} />);
  expect(screen.getAllByRole("button").map((button) => button.getAttribute("aria-label"))).toEqual(["Clear Loop"]);
  fireEvent.click(screen.getByRole("button", { name: "Clear Loop" }));
  expect(onAction).toHaveBeenLastCalledWith({ target: "loop", action: { type: "clear", loopId: task.value.id } });
});
