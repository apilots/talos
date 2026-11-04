// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  getTimelineEventsMocks, historyOptions, resetTimelineEventsTest, cleanupTimelineEventsTest,
} from "./useSessionTimelineEvents.test-support";
import { useSessionTimelineEvents } from "./useSessionTimelineEvents";
import type { SessionHistoryResponse } from "@talos/sdk-core";
import { useAppStore } from "@/shared/store/app-store";
import { sessionHistory, textDelta } from "../__tests__/streaming-fixtures";
import { runningTurn } from "../__tests__/running-turn-fixture";
import { restoreActiveTurn } from "../stream-events";

const apiMocks = getTimelineEventsMocks();

describe("running Turn query to incremental subscription", () => {
  afterEach(cleanupTimelineEventsTest);
  beforeEach(() => {
    resetTimelineEventsTest();
    apiMocks.subscribeTimelineEvents.mockImplementation(() => new Promise(() => undefined));
    apiMocks.getSessionHistory.mockResolvedValue({ data: [], nextCursor: null, lastStableEventId: null });
  });

  it("restores the queried record before subscribing after its offset", async () => {
    renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, undefined, { ...historyOptions, activeTurnState: { activeTurn: runningTurn(), lastCompletedEventSeq: 0, latestEventId: "ev_9" } }));
    expect(apiMocks.subscribeTimelineEvents).not.toHaveBeenCalled();
    await act(async () => { await Promise.resolve(); });
    expect(apiMocks.getSessionHistory).not.toHaveBeenCalled();
    expect(apiMocks.subscribeTimelineEvents.mock.calls[0]?.[3]).toBe("ev_9");
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({ isStreaming: true, lastEventSeq: 9 });
    act(() => apiMocks.subscribeTimelineEvents.mock.calls[0]?.[1](textDelta("answer", "tail", 10)));
    expect(useAppStore.getState().streams["sess-1"]?.lastEventId).toBe("ev_10");
  });

  it("uses completed history when the captured Turn finishes during restoration", async () => {
    const history = sessionHistory([]);
    renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, undefined, { ...historyOptions, history, activeTurnState: { activeTurn: runningTurn(), lastCompletedEventSeq: 0, latestEventId: "ev_9" } }));
    await act(async () => { await Promise.resolve(); });
    expect(apiMocks.subscribeTimelineEvents.mock.calls[0]?.[3]).toBe(history.lastStableEventId);
    expect(useAppStore.getState().streams["sess-1"]?.isStreaming).toBe(false);
  });

  it("refreshes completed history before subscribing when the active query observes idle", async () => {
    const history = sessionHistory([]);
    apiMocks.getSessionHistory.mockResolvedValue(history);
    renderHook(() => useSessionTimelineEvents("sess-1", "older-history", true, undefined, undefined,
      { ...historyOptions, activeTurnState: { activeTurn: null, lastCompletedEventSeq: history.data[0].completedEventSeq, latestEventId: "ev_11" } }));
    expect(apiMocks.subscribeTimelineEvents).not.toHaveBeenCalled();
    await act(async () => { await Promise.resolve(); });
    expect(apiMocks.subscribeTimelineEvents.mock.calls[0]?.[3]).toBe(history.lastStableEventId);
  });

  it("does not restore or subscribe after navigation while history is pending", async () => {
    let resolve!: (history: SessionHistoryResponse) => void;
    apiMocks.getSessionHistory.mockReturnValue(new Promise<SessionHistoryResponse>((done) => { resolve = done; }));
    const hook = renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, undefined, { ...historyOptions, activeTurnState: { activeTurn: runningTurn(), lastCompletedEventSeq: 5, latestEventId: "ev_9" } }));
    hook.unmount();
    await act(async () => { resolve({ data: [], nextCursor: null, lastStableEventId: null }); });
    expect(apiMocks.subscribeTimelineEvents).not.toHaveBeenCalled();
    expect(useAppStore.getState().streams["sess-1"]?.isStreaming).toBe(false);
  });

  it("refreshes History only when a preceding Turn completed between parallel reads", async () => {
    const history = sessionHistory([]);
    history.data[0].identity.turnId = "preceding-turn";
    apiMocks.getSessionHistory.mockResolvedValue(history);
    renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, undefined,
      { ...historyOptions, activeTurnState: { activeTurn: runningTurn(),
        lastCompletedEventSeq: history.data[0].completedEventSeq, latestEventId: "ev_9" } }));
    await act(async () => { await Promise.resolve(); });
    expect(apiMocks.getSessionHistory).toHaveBeenCalledOnce();
    expect(apiMocks.subscribeTimelineEvents.mock.calls[0]?.[3]).toBe("ev_9");
  });

  it("reuses matching idle History without querying it again", async () => {
    const history = sessionHistory([]);
    renderHook(() => useSessionTimelineEvents("sess-1", history.lastStableEventId!, true, undefined, undefined,
      { ...historyOptions, history, activeTurnState: { activeTurn: null,
        lastCompletedEventSeq: history.data[0].completedEventSeq, latestEventId: history.lastStableEventId } }));
    await act(async () => { await Promise.resolve(); });
    expect(apiMocks.getSessionHistory).not.toHaveBeenCalled();
    expect(apiMocks.subscribeTimelineEvents.mock.calls[0]?.[3]).toBe(history.lastStableEventId);
  });

  it("reconnects from committed state when a later queued delta was interrupted", async () => {
    vi.useFakeTimers();
    restoreActiveTurn("sess-1", runningTurn());
    let reject!: (error: Error) => void;
    apiMocks.subscribeTimelineEvents.mockImplementationOnce(() => new Promise((_done, fail) => { reject = fail; }));
    renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, undefined, { ...historyOptions, activeTurnState: { activeTurn: runningTurn(), lastCompletedEventSeq: 0, latestEventId: "ev_9" } }));
    act(() => {
      const onEvent = apiMocks.subscribeTimelineEvents.mock.calls[0]?.[1];
      onEvent(textDelta("answer", "one", 10));
      onEvent(textDelta("answer", "two", 11));
    });
    await act(async () => { reject(new Error("connection lost")); await Promise.resolve(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(apiMocks.subscribeTimelineEvents.mock.calls[1]?.[3]).toBe("ev_10");
    act(() => {
      apiMocks.subscribeTimelineEvents.mock.calls[0]?.[1](textDelta("answer", "stale connection", 12));
      apiMocks.subscribeTimelineEvents.mock.calls[1]?.[1](textDelta("answer", "two", 11));
    });
    const blocks = useAppStore.getState().streams["sess-1"]?.blocks;
    expect(JSON.stringify(blocks)).toContain("baseonetwo");
    expect(useAppStore.getState().streams["sess-1"]?.lastEventSeq).toBe(11);
  });

  it("keeps the committed live cursor when the cached Turn query still reports idle", () => {
    restoreActiveTurn("sess-1", runningTurn());
    renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, undefined,
      { ...historyOptions, activeTurnState: {
        activeTurn: null, lastCompletedEventSeq: 0, latestEventId: null,
      } }));
    expect(apiMocks.subscribeTimelineEvents.mock.calls[0]?.[3]).toBe("ev_9");
    expect(apiMocks.getSessionHistory).not.toHaveBeenCalled();
  });
});
