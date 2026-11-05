// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { getTimelineEventsMocks, historyOptions, resetTimelineEventsTest, cleanupTimelineEventsTest } from "./useSessionTimelineEvents.test-support";
import { useSessionTimelineEvents } from "./useSessionTimelineEvents";
import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";
import { useSessionAutomation } from "@/features/automation/useSessionAutomation";
import { loop } from "@/features/automation/automation.test-support";
import { setTalosClientFactory, type TalosWebClient } from "@/shared/api/client";

const api = getTimelineEventsMocks();
beforeEach(resetTimelineEventsTest);
afterEach(() => { cleanup(); cleanupTimelineEventsTest(); });
it("removes stopped Loops after reconnect even when their metadata notification was missed and no canonical event follows", async () => {
  vi.useFakeTimers();
  let stopped = false;
  const read = vi.fn(async () => ({ sessionId: "sess-1", goal: null, loops: stopped ? [] : [loop({ sessionId: "sess-1" })] }));
  setTalosClientFactory(() => ({ automation: { read, commandCatalog: vi.fn().mockResolvedValue({ data: [] }) } }) as unknown as TalosWebClient);
  const failures: Array<(error: Error) => void> = [];
  api.subscribeTimelineEvents.mockImplementation((_session, _onEvent, _signal, _cursor, options) => {
    options.onOpen();
    return new Promise((_resolve, reject) => { failures.push(reject); });
  });
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const Wrapper = ({ children }: PropsWithChildren) => <QueryClientProvider client={cache}>{children}</QueryClientProvider>;
  const { result } = renderHook(() => {
    const automation = useSessionAutomation("sess-1", true);
    const { invalidateSessionQueries } = useSessionQueryInvalidation("sess-1");
    useSessionTimelineEvents("sess-1", "event-terminal", true, invalidateSessionQueries, undefined, historyOptions);
    return automation;
  }, { wrapper: Wrapper });
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  expect(result.current.tasks[0]?.value.status).toBe("active");
  const initialReads = read.mock.calls.length;
  await act(async () => { failures[0]!(new Error("connection lost")); await Promise.resolve(); });
  // The canonical terminal event was consumed already; ModelStop metadata is
  // lost while disconnected. Both reconnection streams deliver no events.
  stopped = true;
  await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
  await act(async () => { await vi.advanceTimersByTimeAsync(1); });
  expect(api.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
  expect(read.mock.calls.length).toBeGreaterThan(initialReads);
  expect(result.current.tasks).toEqual([]);
});

it.each(["goal", "loop"] as const)("refreshes a created %s definition before the running-Turn snapshot finishes", async (mode) => {
  let created = false;
  const read = vi.fn(async () => ({ sessionId: "sess-1", goal: created && mode === "goal" ? {
    id: "goal-new", sessionId: "sess-1", objective: "New objective", objectiveRevision: 1, status: "active", pauseReason: null, reason: null,
    tokenBudget: null, tokensUsed: 0, latestTurnId: null, completionEvidence: null, createdAt: 1, updatedAt: 1,
  } : null, loops: created && mode === "loop" ? [loop({ sessionId: "sess-1" })] : [] }));
  setTalosClientFactory(() => ({ automation: { read, commandCatalog: vi.fn().mockResolvedValue({ data: [] }) } }) as unknown as TalosWebClient);
  api.subscribeTimelineEvents.mockImplementation(() => new Promise(() => undefined));
  const cache = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const Wrapper = ({ children }: PropsWithChildren) => <QueryClientProvider client={cache}>{children}</QueryClientProvider>;
  const { result } = renderHook(() => {
    const automation = useSessionAutomation("sess-1", true);
    const { invalidateSessionQueries } = useSessionQueryInvalidation("sess-1");
    useSessionTimelineEvents("sess-1", undefined, true, invalidateSessionQueries, undefined, historyOptions);
    return automation;
  }, { wrapper: Wrapper });
  await waitFor(() => expect(cache.isFetching()).toBe(0));
  expect(result.current.tasks).toEqual([]);
  let finishActive!: (value: { activeTurn: null; lastCompletedEventSeq: number; latestEventId: null }) => void;
  let activeResolved = false;
  const active = new Promise((resolve) => { finishActive = resolve; }).then((value) => { activeResolved = true; return value; });
  api.getActiveTurn.mockReturnValueOnce(active);
  created = true;
  const options = api.subscribeTimelineEvents.mock.calls[0]![4];
  try {
    act(() => options.onAutomationChanged({ sessionId: "sess-1" }));
    await waitFor(() => expect(result.current.tasks[0]?.type).toBe(mode));
    expect(activeResolved).toBe(false);
    expect(read).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => { finishActive({ activeTurn: null, lastCompletedEventSeq: 0, latestEventId: null }); });
  }
});

it("does not start metadata refresh from an open callback after its Session effect retires", () => {
  const refresh = vi.fn();
  api.subscribeTimelineEvents.mockImplementation(() => new Promise(() => undefined));
  const { unmount } = renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, refresh, undefined, historyOptions));
  const options = api.subscribeTimelineEvents.mock.calls[0]![4];
  unmount();
  act(() => options.onOpen());
  expect(api.getActiveTurn).not.toHaveBeenCalled();
  expect(refresh).not.toHaveBeenCalled();
});
