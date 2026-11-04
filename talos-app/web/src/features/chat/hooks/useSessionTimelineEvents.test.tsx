// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  getTimelineEventsMocks, historyOptions, resetTimelineEventsTest, cleanupTimelineEventsTest,
} from "./useSessionTimelineEvents.test-support";
import { useSessionTimelineEvents } from "./useSessionTimelineEvents";

import {
  activeTurn,
  sessionHistory,
  contextCompactionStarted,
  turnCompleted,
  userInputStarted,
  waitingForUserAction,
} from "../__tests__/streaming-fixtures";
import type { SessionHistoryResponse } from "@talos/sdk-core";
import { activeContextCompaction } from "../timeline-state";

import { useSessionMessageQueue } from "./useSessionMessageQueue";
import { setTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import { useAppStore } from "@/shared/store/app-store";
import {
  AppProtocol,
  TalosApiError,
  TalosStreamTerminatedError,
  type TimelineEvent,
} from "@talos/sdk-browser";

const apiMocks = getTimelineEventsMocks();

describe("useSessionTimelineEvents", () => {
  beforeEach(resetTimelineEventsTest);
  afterEach(cleanupTimelineEventsTest);

  it("applies session-scoped compaction events after the stable cursor", () => {
    renderHook(() => useSessionTimelineEvents("sess-1", "event-4", true, undefined, undefined, historyOptions));
    const [sessionId, onEvent, signal, afterEventId] = apiMocks.subscribeTimelineEvents.mock.calls[0] as [
      string,
      (event: TimelineEvent) => void,
      AbortSignal,
      string,
    ];

    expect({ sessionId, afterEventId, aborted: signal.aborted }).toEqual({
      sessionId: "sess-1",
      afterEventId: "event-4",
      aborted: false,
    });
    act(() => onEvent(contextCompactionStarted("compact-1", "preparing", "Preparing.", 5)));

    expect(activeContextCompaction(useAppStore.getState().streams["sess-1"])?.stage).toBe("preparing");
  });

  it("uses the same connection for queue open, notifications, and disconnect", async () => {
    vi.useFakeTimers();
    const queue = { onOpen: vi.fn(), onMessageQueueChanged: vi.fn(), onDisconnect: vi.fn() };
    apiMocks.subscribeTimelineEvents.mockImplementationOnce(async (_sessionId, _onEvent, _signal, _cursor, options) => {
      options.onOpen();
      options.onMessageQueueChanged({ sessionId: "sess-1", revision: 2 });
      throw new Error("connection lost");
    });
    renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, queue, historyOptions));
    await act(async () => { await Promise.resolve(); });
    expect(queue.onOpen).toHaveBeenCalledOnce();
    expect(queue.onMessageQueueChanged).toHaveBeenCalledWith({ sessionId: "sess-1", revision: 2 });
    expect(queue.onDisconnect).toHaveBeenCalledOnce();
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledOnce();
  });

  it("requests authoritative reconciliation after a terminal turn", () => {
    const onTerminalTurn = vi.fn();
    renderHook(() =>
      useSessionTimelineEvents("sess-1", "event-4", true, onTerminalTurn, undefined, historyOptions),
    );
    const onEvent = apiMocks.subscribeTimelineEvents.mock.calls[0]?.[1] as
      | ((event: TimelineEvent) => void)
      | undefined;

    expect(onEvent).toBeDefined();
    act(() => onEvent?.(turnCompleted(5)));

    expect(onTerminalTurn).toHaveBeenCalledTimes(1);
  });

  it("reconnects the shared connection when queue reconciliation fails", async () => {
    vi.useFakeTimers();
    const queue = { revision: 2, maxMessages: 10, messages: [] };
    const list = vi.fn().mockRejectedValueOnce(new Error("queue unavailable")).mockResolvedValue(queue);
    setTalosClientFactory(() => ({ messageQueue: { list } }) as unknown as TalosWebClient);
    apiMocks.subscribeTimelineEvents.mockImplementation((_sessionId, _onEvent, signal, _cursor, options) => {
      options.onOpen();
      return new Promise((_resolve, reject) => {
        signal.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")));
      });
    });
    const { result } = renderHook(() => {
      const messages = useSessionMessageQueue("sess-1");
      useSessionTimelineEvents("sess-1", undefined, true, undefined, messages, historyOptions);
      return messages;
    });
    await act(async () => { await Promise.resolve(); await Promise.resolve(); });
    expect(result.current.error).toBe("queue unavailable");
    await act(() => vi.advanceTimersByTimeAsync(2_000));
    expect({ queue: result.current.queue, error: result.current.error }).toEqual({ queue, error: undefined });
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
  });

  it("ignores a late queue reconciliation failure from a retired connection", async () => {
    vi.useFakeTimers();
    const failures: Array<(error: unknown) => void> = [];
    const queue = { onOpen: (fail: (error: unknown) => void) => failures.push(fail), onDisconnect: vi.fn(), onMessageQueueChanged: vi.fn() };
    apiMocks.subscribeTimelineEvents
      .mockImplementationOnce(async (_id, _event, _signal, _cursor, options) => {
        options.onOpen(); throw new Error("first connection closed");
      })
      .mockImplementationOnce((_id, _event, _signal, _cursor, options) => {
        options.onOpen(); return new Promise(() => undefined);
      });
    renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, queue, historyOptions));
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(failures).toHaveLength(2);
    act(() => failures[0](new Error("old queue request failed")));
    expect(apiMocks.subscribeTimelineEvents.mock.calls[1]?.[2].aborted).toBe(false);
  });

  it("does not reconcile stable history while a turn waits for user action", () => {
    const onTerminalTurn = vi.fn();
    renderHook(() =>
      useSessionTimelineEvents("sess-1", "event-4", true, onTerminalTurn, undefined, historyOptions),
    );
    const onEvent = apiMocks.subscribeTimelineEvents.mock.calls[0]?.[1] as
      | ((event: TimelineEvent) => void)
      | undefined;

    act(() => onEvent?.(waitingForUserAction(5)));

    expect(onTerminalTurn).not.toHaveBeenCalled();
  });

  it("reconnects a failed subscription from the last received event", async () => {
    vi.useFakeTimers();
    apiMocks.subscribeTimelineEvents
      .mockRejectedValueOnce(new Error("stream disconnected"))
      .mockImplementationOnce(() => new Promise(() => undefined));
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "message-1",
      isStreaming: true,
      lastEventId: "event-7",
    });

    renderHook(() => useSessionTimelineEvents("sess-1", "event-4", true, undefined, undefined, historyOptions));
    await Promise.resolve();
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(1);
    await act(() => vi.advanceTimersByTimeAsync(2_000));

    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
    expect(apiMocks.subscribeTimelineEvents.mock.calls[1]?.[3]).toBe("event-7");
  });

  it("recovers the server cursor before resubscribing after retention", async () => {
    apiMocks.subscribeTimelineEvents
      .mockRejectedValueOnce(
        new TalosApiError({
          code: AppProtocol.ErrorCode.EventCursorExpired, localization: null,
          message: "Event cursor expired",
        }),
      )
      .mockImplementationOnce(() => new Promise(() => undefined));
    apiMocks.getActiveTurn.mockResolvedValue({ activeTurn: activeTurn({
      initialMessageId: "message-1", lastEventId: "event-9", lastEventSeq: 9, lastSequence: 9,
    }), lastCompletedEventSeq: 0, latestEventId: "event-9" });
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "message-1",
      isStreaming: true,
      connectionState: "streaming",
      lastEventId: "event-1",
    });

    renderHook(() => useSessionTimelineEvents("sess-1", "event-1", true, undefined, undefined, historyOptions));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(apiMocks.getSessionHistory).toHaveBeenCalledWith("sess-1");
    expect(apiMocks.getActiveTurn).toHaveBeenCalledWith("sess-1");
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
    expect(apiMocks.subscribeTimelineEvents.mock.calls[1]?.[3]).toBe("event-9");
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeMessageId: "message-1",
      isStreaming: true,
      lastEventId: "event-9",
      error: undefined,
    });
  });

  it("recovers a lagged terminal and remains subscribed for the next Turn", async () => {
    apiMocks.subscribeTimelineEvents
      .mockRejectedValueOnce(
        new TalosStreamTerminatedError({
          reason: "lagged",
          message: "subscription lagged behind retained live events",
        }),
      )
      .mockImplementationOnce(() => new Promise(() => undefined));
    apiMocks.getActiveTurn.mockResolvedValue({ activeTurn: null, lastCompletedEventSeq: 11, latestEventId: "event-11" });
    const completedHistory = sessionHistory([]);
    completedHistory.data[0].completedEventSeq = 11;
    completedHistory.lastStableEventId = "ev_11";
    apiMocks.getSessionHistory.mockResolvedValue(completedHistory);
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "message-1",
      isStreaming: true,
      connectionState: "streaming",
      lastEventId: "event-7",
    });

    renderHook(() => useSessionTimelineEvents("sess-1", "event-4", true, undefined, undefined, historyOptions));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(apiMocks.getActiveTurn).toHaveBeenCalledWith("sess-1");
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
    expect(apiMocks.subscribeTimelineEvents.mock.calls[1]?.[3]).toBe("ev_11");
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeMessageId: undefined,
      isStreaming: false,
      lastEventId: "ev_11",
      error: undefined,
    });
    const onEvent = apiMocks.subscribeTimelineEvents.mock.calls[1]?.[1] as (event: TimelineEvent) => void;
    act(() => onEvent(userInputStarted("next", "next question", 12)));
    expect(useAppStore.getState().streams["sess-1"]?.isStreaming).toBe(true);
  });

  it("continues reconnecting at a fixed interval after the initial attempts", async () => {
    vi.useFakeTimers();
    apiMocks.subscribeTimelineEvents.mockRejectedValue(new Error("stream disconnected"));

    const { unmount } = renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, undefined, historyOptions));
    await act(async () => { await Promise.resolve(); });
    for (let retry = 1; retry <= 7; retry += 1) {
      await act(() => vi.advanceTimersByTimeAsync(1_999));
      expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(retry);
      await act(() => vi.advanceTimersByTimeAsync(1));
      expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(retry + 1);
    }
    unmount();
    await act(() => vi.advanceTimersByTimeAsync(2_000));
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(8);
  });

  it("does not refetch stale History after navigation during parallel retention recovery", async () => {
    let resolve!: (history: SessionHistoryResponse) => void;
    apiMocks.subscribeTimelineEvents.mockRejectedValueOnce(new TalosStreamTerminatedError({
      reason: "lagged", message: "subscription lagged",
    }));
    apiMocks.getActiveTurn.mockResolvedValue({ activeTurn: null, lastCompletedEventSeq: 9, latestEventId: "ev_9" });
    apiMocks.getSessionHistory.mockReturnValue(new Promise<SessionHistoryResponse>((done) => { resolve = done; }));
    const hook = renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, undefined, historyOptions));
    await act(async () => { await Promise.resolve(); });
    expect(apiMocks.getSessionHistory).toHaveBeenCalledOnce();
    hook.unmount();
    await act(async () => { resolve(historyOptions.history); });
    expect(apiMocks.getSessionHistory).toHaveBeenCalledOnce();
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledOnce();
  });
});
