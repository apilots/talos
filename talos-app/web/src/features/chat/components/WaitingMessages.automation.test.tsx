// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { MessageQueue } from "@talos/sdk-browser";
import { automationController, loop } from "@/features/automation/automation.test-support";
import { WaitingMessages } from "./WaitingMessages";
afterEach(cleanup);
it("merges a triggered Loop into its task row while retaining its canonical queue ID for reorder", async () => {
  const reorder = vi.fn().mockResolvedValue(undefined);
  const queue: MessageQueue = { revision: 3, maxMessages: 10, messages: [
    { id: "human-1", source: { type: "user" }, input: { content: [{ type: "text", text: "Human message" }] } },
    { id: "human-2", source: { type: "user" }, input: { content: [{ type: "text", text: "Enable later" }, { type: "skillSelection", name: "loop", path: "/system/loop/SKILL.md" }] } },
    { id: "auto-1", source: { type: "loop", loopId: "loop-1", occurrenceId: "occ-1" }, input: { content: [{ type: "text", text: "Check CI" }] } },
  ] };
  const automation = automationController({ tasks: [{ type: "loop", value: loop() }] });
  render(<WaitingMessages automation={automation} controller={{ queue, activeTurnId: "turn-1", reorder, steer: vi.fn(), start: vi.fn(), update: vi.fn(), delete: vi.fn() }} />);
  expect(screen.getAllByText("Check CI")).toHaveLength(1);
  expect(screen.getByText("3/10")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Steer" })).toHaveLength(1);
  expect(screen.getAllByRole("button", { name: "More message actions" })).toHaveLength(2);
  fireEvent.pointerDown(screen.getAllByRole("button", { name: "More message actions" })[0]!, { button: 0, ctrlKey: false });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Move down" }));
  expect(reorder).toHaveBeenCalledWith(["human-2", "human-1", "auto-1"]);
  fireEvent.click(screen.getByRole("button", { name: "Stop Loop" }));
  await waitFor(() => expect(automation.execute).toHaveBeenCalledWith({ target: "loop", action: { type: "stop", loopId: "loop-1" } }));
  const toggle = screen.getByRole("button", { name: /Tasks and waiting messages/ });
  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("button", { name: "Steer" })).not.toBeInTheDocument();
});
it("does not offer Steer around an outstanding human action", () => {
  render(<WaitingMessages controller={{ activeTurnId: "turn-1", canSteer: false, queue: { revision: 1, maxMessages: 10, messages: [{ id: "u1", source: { type: "user" }, input: { content: [{ type: "text", text: "Waiting" }] } }] }, reorder: vi.fn(), steer: vi.fn(), start: vi.fn(), update: vi.fn(), delete: vi.fn() }} />);
  expect(screen.queryByRole("button", { name: "Steer" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Start queued message" })).not.toBeInTheDocument();
});

it("shows initial waiting only for the Loop's exact original queue item without duplicating the row", () => {
  const automation = automationController({ tasks: [{ type: "loop", value: loop() }] });
  const controller = { queue: { revision: 1, maxMessages: 10, messages: [
    { id: "auto-other", source: { type: "loop" as const, loopId: "another-loop", occurrenceId: "other" }, input: { content: [{ type: "text" as const, text: "Other work" }] } },
  ] }, reorder: vi.fn(), steer: vi.fn(), start: vi.fn(), update: vi.fn(), delete: vi.fn() };
  const view = render(<WaitingMessages automation={automation} controller={controller} />);
  expect(screen.queryByText("Waiting")).not.toBeInTheDocument();
  view.rerender(<WaitingMessages automation={automation} controller={{ ...controller, queue: { ...controller.queue, messages: [
    { id: "auto-matching", source: { type: "loop", loopId: "loop-1", occurrenceId: "first" }, input: { content: [{ type: "text", text: "Check CI" }] } },
  ] } }} />);
  expect(screen.getByText("Waiting")).toBeVisible();
  expect(screen.getAllByText("Check CI")).toHaveLength(1);
  expect(screen.getByText("1/10")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Cancel Loop" }));
  expect(automation.execute).toHaveBeenCalledExactlyOnceWith({ target: "loop", action: { type: "clear", loopId: "loop-1" } });
});
