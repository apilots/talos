// @vitest-environment jsdom
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { installTracingDom, cleanupTracingDom } from "./tracing.test-utils";
import { i18n } from "@/shared/i18n/setup";
import { SessionTracingPanel } from "./components/SessionTracingPanel";
import { history, request } from "./tracing.fixtures";

const mocks = vi.hoisted(() => ({
  capabilities: vi.fn(),
  readTurn: vi.fn(),
  readRequest: vi.fn(),
}));
vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({ trajectory: mocks }),
}));
vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "range-connection",
}));
beforeEach(async () => {
  await i18n.changeLanguage("en");
  installTracingDom({ narrow: true });
  mocks.capabilities.mockResolvedValue({
    supported: true,
  });
  mocks.readTurn.mockImplementation(async ({ identity, cursor }) => ({
    identity,
    clockSegments:
      identity.turnId === "turn-4"
        ? []
        : [
            {
              segmentId: identity.turnId,
              sourceNodeId: "worker",
              startedAt: 100 + Number(identity.turnId.split("-").at(-1) || 1) * 5,
              relation: "turnStart",
              observedThroughOffsetMs: 2000,
            },
          ],
    data:
      identity.turnId === "turn-4"
        ? null
        : [
            {
              type: "modelRequest",
              request: {
                ...request,
                modelRequestId: `${identity.turnId}/${cursor ?? "first"}`,
                ordinal: cursor ? 2 : 1,
                callRecords: request.callRecords.map((a) => ({
                  ...a,
                  timing: {
                    ...a.timing!,
                    interval: {
                      ...a.timing!.interval,
                      segmentId: identity.turnId,
                    },
                  },
                })),
              },
            },
          ],
    nextCursor: identity.turnId === "turn-1" && !cursor ? "more" : null,
  }));
  const missing = null;
  mocks.readRequest.mockImplementation(async ({ identity, modelRequestId }) => ({
    identity,
    request: { ...request, modelRequestId },
    prompt: missing,
    tools: missing,
    parameters: missing,
    outputs: [],
  }));
});
afterEach(() => {
  cleanupTracingDom();
});

it("shows all Turn intervals on one Session timeline and opens their exact details", async () => {
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const savedHistory = { ...history, data: [1, 2, 3, 4].map((n) => {
    const turn = structuredClone(history.data[0]);
    turn.identity = { sessionId: "s", turnId: `turn-${n}` };
    turn.result.result.metrics.durationMs = n * 1500;
    return turn;
  }) };
  const panel = (data = savedHistory) => (
    <QueryClientProvider client={cache}>
      <SessionTracingPanel sessionId="s" visible history={data} />
    </QueryClientProvider>
  );
  const { rerender } = render(panel());
  const ledger = screen.getByLabelText("Continuous execution records");
  ["1.5 s", "3 s", "4.5 s", "6 s"].forEach((duration, index) => {
    const header = within(ledger.querySelectorAll<HTMLElement>(".tracing-turn > summary")[index]);
    expect(header.getByText(`Turn ${index + 1}`)).toBeTruthy();
    expect(header.getByText(duration)).toBeTruthy();
  });
  const overview = await screen.findByLabelText("Session execution timeline");
  await within(overview).findByRole("button", { name: "Turn 1 · LLM · Model request 2 · Call 1" });
  expect(new Set(mocks.readTurn.mock.calls.map(([args]) => args.identity.turnId))).toEqual(new Set(["turn-1", "turn-2", "turn-3", "turn-4"]));
  for (const ordinal of [1, 2, 3]) {
    expect(within(overview).getByRole("button", { name: `Turn ${ordinal} · LLM · Model request 1 · Call 1` })).toBeTruthy();
  }
  expect(screen.getByText("Some Turns have no saved timing; their existing history remains available in the ledger.")).toBeTruthy();
  expect(screen.queryByText("Selected Turn timing")).toBeNull();
  expect(ledger.querySelector('.tracing-turn:not([open]) .tracing-feedback')).toBeNull();
  const compact = within(overview).getByRole("checkbox", { name: "Collapse idle between Turns" });
  expect((compact as HTMLInputElement).checked).toBe(true);
  fireEvent.click(compact);
  expect(within(overview).getByText("Recorded start times · gaps retained")).toBeTruthy();
  fireEvent.click(compact);
  expect(mocks.readRequest).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("combobox", { name: "View" }));
  fireEvent.click(await screen.findByRole("option", { name: "Turn 2 · Completed" }));
  await screen.findByText("Turn 2 · original totals");
  expect(within(ledger).getByText(/^Turn 2$/)).toBeTruthy();
  expect(within(ledger).queryByText(/^Turn 1$/)).toBeNull();
  expect(screen.getByText("Turn 2 · original totals")).toBeTruthy();
  expect(within(overview).getByRole("button", { name: "Turn 3 · LLM · Model request 1 · Call 1" })).toBeTruthy();
  fireEvent.click(screen.getByRole("combobox", { name: "View" }));
  fireEvent.click(await screen.findByRole("option", { name: "All loaded Turns" }));

  await waitFor(() => expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "View" })));
  const source = within(overview).getByRole("button", { name: "Turn 1 · LLM · Model request 2 · Call 1" });
  source.focus();
  fireEvent.click(source);
  expect(mocks.readTurn.mock.calls.some(([args]) => args.identity.turnId === "turn-1" && args.cursor === "more")).toBe(true);
  await waitFor(() => expect(mocks.readRequest).toHaveBeenCalledWith(
    { identity: { sessionId: "s", turnId: "turn-1" }, modelRequestId: "turn-1/more" }, expect.any(AbortSignal),
  ));
  expect(screen.getByText("Turn 1 · original totals")).toBeTruthy();
  fireEvent.keyDown(screen.getByRole("button", { name: "Close trace details" }), { key: "Escape" });
  expect(document.activeElement).toBe(source);

  fireEvent.click(screen.getByRole("combobox", { name: "View" }));
  fireEvent.click(await screen.findByRole("option", { name: "Turn 1 · Completed" }));
  fireEvent.click(within(ledger).getAllByRole("button", { name: /LLM.*Call 1/ })[0]);
  const replacement = structuredClone(history.data[0]);
  replacement.identity.turnId = "after-clear";
  rerender(panel({ ...history, data: [replacement] }));
  expect(screen.getByRole("combobox", { name: "View" }).textContent).toBe("All loaded Turns");
  expect(screen.queryByRole("tab", { name: "Summary" })).toBeNull();
  await screen.findByText("Turn 1 · original totals");
  expect(mocks.readRequest.mock.calls.some(([args]) => args.identity.turnId === "after-clear")).toBe(false);

  const manyTurns = Array.from({ length: 80 }, (_, index) => {
    const item = structuredClone(history.data[0]);
    item.identity = { sessionId: "s", turnId: `many-${index + 1}` };
    return item;
  });
  rerender(panel({ ...history, data: manyTurns }));
  await waitFor(() => expect(mocks.readTurn.mock.calls.some(([args]) => args.identity.turnId === "many-80")).toBe(true));
  expect(ledger.querySelectorAll(".tracing-turn > summary")).toHaveLength(80);
  expect(within(overview).queryByRole("button", { name: "Turn 80 · LLM · Model request 1 · Call 1" })).toBeNull();
  const timeline = within(overview).getByRole("region", { name: "Session timeline, scroll horizontally to inspect all calls" });
  const keyboardSource = within(overview).getByRole("button", { name: "Turn 1 · LLM · Model request 1 · Call 1" });
  act(() => keyboardSource.focus());
  fireEvent.scroll(timeline, { target: { scrollLeft: 1500 } });
  expect(keyboardSource.isConnected).toBe(true);
  expect(document.activeElement).toBe(keyboardSource);
  act(() => timeline.focus());
  expect(keyboardSource.isConnected).toBe(false);
  const lastSource = await within(overview).findByRole("button", { name: "Turn 80 · LLM · Model request 1 · Call 1" });
  act(() => lastSource.focus());
  fireEvent.click(lastSource);
  fireEvent.scroll(timeline, { target: { scrollLeft: 0 } });
  fireEvent.keyDown(screen.getByRole("button", { name: "Close trace details" }), { key: "Escape" });
  expect(lastSource.isConnected).toBe(true);
  expect(document.activeElement).toBe(lastSource);
});
