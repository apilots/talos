// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { setTalosClientFactory, resetTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import type { AppProtocol } from "@talos/sdk-browser";
const loop = (): AppProtocol.SessionLoop => ({ id: "loop-1", sessionId: "s1", prompt: "Check CI", schedule: { type: "modelDecided" }, configRevision: 3, status: "active", stopReason: null, reason: null, nextDueAt: null, latestTurnId: null, createdAt: 1, updatedAt: 2 });
const goal = (extra: Partial<AppProtocol.SessionGoal>): AppProtocol.SessionGoal => ({ id: "goal-1", sessionId: "s1", objective: "Fix CI", objectiveRevision: 2, status: "active", pauseReason: null, reason: null, tokenBudget: 2000, tokensUsed: 100, latestTurnId: null, completionEvidence: null, createdAt: 1, updatedAt: 2, ...extra });
import { AutomationPage } from "./AutomationPage";
vi.mock("@/features/computers", () => ({ useProductScope: () => "account-scope", useComputers: () => ({ selectedId: "computer-A", selected: { displayName: "My Computer" } }) }));
afterEach(() => { cleanup(); resetTalosClientFactory(); });
function setup() {
  const list = vi.fn().mockResolvedValue({ data: [{ type: "loop", loopTask: loop() }], nextCursor: null });
  const execute = vi.fn().mockResolvedValue({});
  const listJobs = vi.fn().mockResolvedValue({ data: [{ id: "profileUsageRetention", computerId: "computer-A", executionInstanceId: "instance-A", name: "Usage history cleanup", schedule: "0 15 3 * * MON", timezone: "Asia/Shanghai", paused: true, running: false, nextDueAt: null, lastFinishedAt: null, lastOutcome: null, availableActions: ["resume", "runOnce"] }] });
  const control = vi.fn().mockResolvedValue({});
  setTalosClientFactory(() => ({ automation: { list, execute }, builtinJobs: { list: listJobs, control } }) as unknown as TalosWebClient);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(<QueryClientProvider client={client}><MemoryRouter><AutomationPage /></MemoryRouter></QueryClientProvider>);
  return { list, execute, listJobs, control };
}
it("lists real Session facts with links and no independent create entry", async () => {
  const mocks = setup();
  expect(await screen.findByText("Check CI")).toBeInTheDocument();
  expect(mocks.list).toHaveBeenCalledWith({ sessionId: null, scope: "unfinished", cursor: null, limit: 30 });
  expect(screen.getByRole("link", { name: /Open chat/ })).toHaveAttribute("href", "/sessions/s1");
  expect(screen.queryByRole("button", { name: /new|create/i })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Stop Loop" }));
  await waitFor(() => expect(mocks.execute).toHaveBeenCalledWith({ sessionId: "s1", action: { target: "loop", action: { type: "stop", loopId: "loop-1" } } }));
});
it("controls only the selected Computer's advertised native job actions, keeping paused Run once separate", async () => {
  const mocks = setup();
  fireEvent.click(screen.getByRole("button", { name: "Built-in jobs" }));
  expect(await screen.findByText("Usage history cleanup")).toBeInTheDocument();
  expect(mocks.listJobs).toHaveBeenCalledWith({ computerId: "computer-A" });
  expect(screen.getByText("Paused")).toBeInTheDocument();
  expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Run once" }));
  await waitFor(() => expect(mocks.control).toHaveBeenCalledWith({ computerId: "computer-A", executionInstanceId: "instance-A", jobId: "profileUsageRetention", action: "runOnce" }));
  expect(screen.getByText("Paused")).toBeInTheDocument();
  expect(mocks.execute).not.toHaveBeenCalled();
});
it("clears a historical Goal by its own ID without targeting the current Goal", async () => {
  const mocks = setup();
  await screen.findByText("Check CI");
  mocks.list.mockResolvedValue({ data: [{ type: "goal", goal: goal({ id: "old-goal", status: "complete" }) }], nextCursor: null });
  fireEvent.click(screen.getByRole("button", { name: "Ended" }));
  await screen.findByText("Fix CI");
  expect(screen.queryByRole("button", { name: "Resume Goal" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Clear Goal" }));
  await waitFor(() => expect(mocks.execute).toHaveBeenCalledWith({ sessionId: "s1", action: { target: "goal", action: { type: "clear", goalId: "old-goal" } } }));
});

it("uses one quiet list row for each ended task instead of repeated state, tokens and Session ID lines", async () => {
  const mocks = setup();
  await screen.findByText("Check CI");
  mocks.list.mockResolvedValue({ data: [
    { type: "goal", goal: goal({ id: "ended-goal", status: "complete", sessionId: "technical-chat-goal", tokensUsed: 987654, tokenBudget: 1234567 }) },
    { type: "loop", loopTask: { ...loop(), id: "ended-loop", status: "stopped", stopReason: "userStopped", sessionId: "technical-chat-loop" } },
  ], nextCursor: null });
  fireEvent.click(screen.getByRole("button", { name: "Ended" }));
  expect(await screen.findByRole("heading", { name: "Fix CI" })).toBeInTheDocument();
  expect(screen.getAllByTestId("automation-list-status")).toHaveLength(2);
  expect(screen.getAllByText("Stopped")).toHaveLength(1);
  expect(screen.getAllByTestId("automation-list-metadata")).toHaveLength(2);
  expect(document.body.textContent).not.toMatch(/987654|1234567|technical-chat-goal|technical-chat-loop/);
  expect(screen.getAllByRole("link")).toHaveLength(2);
});

it("returns focus to the same compact list action after closing the existing editor", async () => {
  setup();
  await screen.findByRole("heading", { name: "Check CI" });
  const trigger = screen.getByRole("button", { name: "Edit Loop" });
  fireEvent.click(trigger);
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Instructions" })).toHaveFocus());
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  await waitFor(() => expect(trigger).toHaveFocus());
});
