// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { contextCompactionStarted } from "../__tests__/streaming-fixtures";
import { activeContextCompactionStage } from "../timeline-state";
import { useSessionTimelineEvents } from "./useSessionTimelineEvents";
import { useAppStore } from "@/shared/store/app-store";
import {
  AppProtocol,
  TalosApiError,
  TalosStreamTerminatedError,
  type TimelineEventEnvelope,
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
  recoverTimelineLiveTail: vi.fn(),
  subscribeTimelineEvents: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../api", () => ({
  recoverTimelineLiveTail: apiMocks.recoverTimelineLiveTail,
  subscribeTimelineEvents: apiMocks.subscribeTimelineEvents,
}));

describe("useSessionTimelineEvents", () => {
  beforeEach(() => {
    apiMocks.subscribeTimelineEvents.mockReset().mockResolvedValue(undefined);
    apiMocks.recoverTimelineLiveTail.mockReset();
    useAppStore.getState().resetStream("sess-1");
  });

  afterEach(() => vi.useRealTimers());

  it("applies session-scoped compaction events after the stable cursor", () => {
    renderHook(() => useSessionTimelineEvents("sess-1", "event-4", true));
    const [sessionId, onEvent, signal, afterEventId] = apiMocks.subscribeTimelineEvents.mock.calls[0] as [
      string,
      (event: TimelineEventEnvelope) => void,
      AbortSignal,
      string,
    ];

    expect({ sessionId, afterEventId, aborted: signal.aborted }).toEqual({
      sessionId: "sess-1",
      afterEventId: "event-4",
      aborted: false,
    });
    act(() => onEvent(contextCompactionStarted("compact-1", "preparing", "Preparing.", 5)));

    expect(activeContextCompactionStage(useAppStore.getState().streams["sess-1"])).toBe("preparing");
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

    renderHook(() => useSessionTimelineEvents("sess-1", "event-4", true));
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
          code: AppProtocol.ErrorCode.EventCursorExpired,
          message: "Event cursor expired",
        }),
      )
      .mockImplementationOnce(() => new Promise(() => undefined));
    apiMocks.recoverTimelineLiveTail.mockResolvedValue({
      replay: {
        events: [],
        high_water_mark_event_id: "event-9",
        last_replay_seq: 9,
        cursor_status: "fromBeginning",
      },
      active_turn: {
        session_id: "sess-1",
        turn_id: "turn-1",
        message_id: "message-1",
        status: "running",
        last_event_id: "event-9",
        pending_user_action: null,
        updated_at: 1,
      },
    });
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "message-1",
      isStreaming: true,
      connectionState: "streaming",
      lastEventId: "event-1",
    });

    renderHook(() => useSessionTimelineEvents("sess-1", "event-1", true));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(apiMocks.recoverTimelineLiveTail).toHaveBeenCalledWith("sess-1");
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
    expect(apiMocks.subscribeTimelineEvents.mock.calls[1]?.[3]).toBe("event-9");
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeMessageId: "message-1",
      isStreaming: true,
      lastEventId: "event-9",
      error: undefined,
    });
  });

  it("recovers a lagged live subscription from the last received event", async () => {
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
        events: [],
        high_water_mark_event_id: "event-11",
        last_replay_seq: 11,
        cursor_status: "replayed",
      },
      active_turn: {
        session_id: "sess-1",
        turn_id: "turn-1",
        message_id: "message-1",
        status: "running",
        last_event_id: "event-11",
        pending_user_action: null,
        updated_at: 1,
      },
    });
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "message-1",
      isStreaming: true,
      connectionState: "streaming",
      lastEventId: "event-7",
    });

    renderHook(() => useSessionTimelineEvents("sess-1", "event-4", true));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(apiMocks.recoverTimelineLiveTail).toHaveBeenCalledWith("sess-1", "event-7");
    expect(apiMocks.subscribeTimelineEvents).toHaveBeenCalledTimes(2);
    expect(apiMocks.subscribeTimelineEvents.mock.calls[1]?.[3]).toBe("event-11");
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeMessageId: "message-1",
      isStreaming: true,
      lastEventId: "event-11",
      error: undefined,
    });
  });

  it("continues reconnecting with the maximum delay after the initial attempts", async () => {
    vi.useFakeTimers();
    apiMocks.subscribeTimelineEvents.mockRejectedValue(new Error("stream disconnected"));

    renderHook(() => useSessionTimelineEvents("sess-1", undefined, true));
    for (const delay of [500, 1_000, 2_000, 4_000, 8_000, 12_000, 12_000]) {
      await act(() => vi.advanceTimersByTimeAsync(delay));
    }

    expect(apiMocks.subscribeTimelineEvents.mock.calls.length).toBeGreaterThan(7);
  });
});
