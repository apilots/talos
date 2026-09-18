// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  contextCompactionStarted,
  turnCompleted,
  userInputStarted,
  waitingForUserAction,
} from "../__tests__/streaming-fixtures";
import { activeContextCompaction } from "../timeline-state";
import { useSessionTimelineEvents } from "./useSessionTimelineEvents";
import { useSessionMessageQueue } from "./useSessionMessageQueue";
import { resetTalosClientFactory, setTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import { useAppStore } from "@/shared/store/app-store";
import {
  AppProtocol,
  TalosApiError,
  TalosStreamTerminatedError,
  type TimelineEvent,
} from "@talos/sdk-browser";

vi.hoisted(() => {
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
    },
  });
});

const apiMocks = vi.hoisted(() => ({
  getSessionHistory: vi.fn(),
  recoverTimelineLiveTail: vi.fn(),
  subscribeTimelineEvents: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../api", () => ({
  recoverTimelineLiveTail: apiMocks.recoverTimelineLiveTail,
  subscribeTimelineEvents: apiMocks.subscribeTimelineEvents,
}));

const historyOptions = {
  stableEventSeq: 0,
  reloadHistory: () => apiMocks.getSessionHistory("sess-1"),
};

describe("useSessionTimelineEvents", () => {
  beforeEach(() => {
    apiMocks.subscribeTimelineEvents.mockReset().mockResolvedValue(undefined);
    apiMocks.recoverTimelineLiveTail.mockReset();
    apiMocks.getSessionHistory.mockReset().mockResolvedValue({ data: [], nextCursor: null, lastStableEventId: "event-4" });
    useAppStore.getState().resetStream("sess-1");
  });

  afterEach(() => { vi.useRealTimers(); resetTalosClientFactory(); });

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
    await act(() => vi.advanceTimersByTimeAsync(500));
    expect({ queue: result.current.queue, error: result.current.error }).toEqual({ queue, error: undefined });
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
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
    await act(() => vi.advanceTimersByTimeAsync(500));

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
    apiMocks.recoverTimelineLiveTail.mockResolvedValue({
      replay: {
        events: [],
        highWaterMarkEventId: "event-9",
        nextCursor: null,
        lastEventSeq: 9,
        cursorStatus: "fromBeginning",
      },
      activeTurn: {
        identity: { sessionId: "sess-1", turnId: "turn-1" },
        initialMessageId: "message-1",
        state: "running",
        lastEventId: "event-9",
        updatedAt: 1,
      },
    });
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
    expect(apiMocks.recoverTimelineLiveTail).toHaveBeenCalledWith("sess-1", "event-4");
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
    expect(apiMocks.subscribeTimelineEvents.mock.calls[1]?.[3]).toBe("event-9");
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeMessageId: "message-1",
      isStreaming: true,
      lastEventId: "event-4",
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
    apiMocks.recoverTimelineLiveTail.mockResolvedValue({
      replay: {
        events: [turnCompleted(11)],
        highWaterMarkEventId: "event-11",
        nextCursor: null,
        lastEventSeq: 11,
        cursorStatus: "replayed",
      },
      activeTurn: null,
    });
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

    expect(apiMocks.recoverTimelineLiveTail).toHaveBeenCalledWith("sess-1", "event-7");
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
    expect(apiMocks.subscribeTimelineEvents.mock.calls[1]?.[3]).toBe("event-11");
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeMessageId: "message-1",
      isStreaming: false,
      lastEventId: "ev_11",
      error: undefined,
    });
    const onEvent = apiMocks.subscribeTimelineEvents.mock.calls[1]?.[1] as (event: TimelineEvent) => void;
    act(() => onEvent(userInputStarted("next", "next question", 12)));
    expect(useAppStore.getState().streams["sess-1"]?.isStreaming).toBe(true);
  });

  it("continues reconnecting with the maximum delay after the initial attempts", async () => {
    vi.useFakeTimers();
    apiMocks.subscribeTimelineEvents.mockRejectedValue(new Error("stream disconnected"));

    renderHook(() => useSessionTimelineEvents("sess-1", undefined, true, undefined, undefined, historyOptions));
    for (const delay of [500, 1_000, 2_000, 4_000, 8_000, 12_000, 12_000]) {
      await act(() => vi.advanceTimersByTimeAsync(delay));
    }

    expect(apiMocks.subscribeTimelineEvents.mock.calls.length).toBeGreaterThan(7);
  });
});
