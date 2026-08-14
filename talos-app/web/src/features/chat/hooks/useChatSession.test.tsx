// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useChatSession } from "./useChatSession";
import { useAppStore } from "@/shared/store/app-store";

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

const mocks = vi.hoisted(() => ({
  activeTurn: undefined as unknown,
  activeTurnQuery: vi.fn(),
  activeTurnRefetch: vi.fn().mockResolvedValue(undefined),
  createInitialTurnRequest: vi.fn(() => ({
    session_id: "session-1",
    input: [{ type: "files", fileIds: ["file-1"] }],
  })),
  send: vi.fn().mockRejectedValue({ code: -32013 }),
  sessionHistoryRefetch: vi.fn().mockResolvedValue(undefined),
  liveRecoveryQuery: vi.fn(),
  timeline: vi.fn(),
}));

vi.mock("../api", () => ({
  createInitialTurnRequest: mocks.createInitialTurnRequest,
  isActiveTurnRunningError: (error: unknown) =>
    Boolean(error && typeof error === "object" && "code" in error && error.code === -32013),
}));

vi.mock("./useChatQueries", () => ({
  useActiveTurnQuery: (...args: unknown[]) => {
    mocks.activeTurnQuery(...args);
    return {
      data: mocks.activeTurn,
      error: undefined,
      refetch: mocks.activeTurnRefetch,
    };
  },
  useLiveRecoveryQuery: (...args: unknown[]) => {
    mocks.liveRecoveryQuery(...args);
    return { data: undefined };
  },
  useSessionHistoryQuery: () => ({
    data: { last_stable_event_id: null, messages: [] },
    error: undefined,
    isSuccess: true,
    refetch: mocks.sessionHistoryRefetch,
  }),
}));

vi.mock("./useSessionBlocks", () => ({
  useSessionBlocks: () => [],
}));

vi.mock("./useSessionTimelineEvents", () => ({
  useSessionTimelineEvents: (...args: unknown[]) => {
    mocks.timeline(...args);
    return { retryTimeline: vi.fn() };
  },
}));

vi.mock("./useSessionStreaming", () => ({
  useSessionStreaming: () => ({
    canRetryResponse: false,
    isCancelling: false,
    isStreaming: false,
    retryResponse: vi.fn(),
    sendMutation: { mutateAsync: mocks.send },
    sendText: vi.fn(),
    stopResponse: vi.fn(),
  }),
}));

describe("useChatSession", () => {
  beforeEach(() => {
    mocks.send.mockClear();
    mocks.createInitialTurnRequest.mockClear();
    mocks.activeTurnRefetch.mockClear();
    mocks.activeTurnQuery.mockClear();
    mocks.sessionHistoryRefetch.mockClear();
    mocks.liveRecoveryQuery.mockClear();
    mocks.timeline.mockClear();
    mocks.activeTurn = undefined;
    useAppStore.getState().resetStream("session-1");
  });

  it("restores the draft when the server reports an active turn", async () => {
    const { result } = renderHook(() => useChatSession("session-1", 0));
    act(() => result.current.setDraft("keep this input"));

    await act(() => result.current.sendTurn());

    expect(mocks.send).toHaveBeenCalledTimes(1);
    expect(result.current.draft).toBe("keep this input");
  });

  it("starts the new session timeline after the server accepts the local turn", () => {
    const { rerender } = renderHook(() => useChatSession("session-1"));

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      false,
      expect.any(Function),
    );

    act(() => {
      useAppStore.getState().setStreamMeta("session-1", {
        isStreaming: true,
      });
    });
    rerender();

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      false,
      expect.any(Function),
    );

    act(() => {
      useAppStore.getState().setStreamMeta("session-1", {
        activeMessageId: "message-1",
        isStreaming: true,
      });
    });
    rerender();

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      true,
      expect.any(Function),
    );
  });

  it("does not poll active turn snapshots while a local event stream is active", () => {
    mocks.activeTurn = {
      session_id: "session-1",
      turn_id: "turn-1",
      message_id: "message-1",
      status: "waiting_user_action",
      last_event_id: "event-1",
      pending_user_action: null,
      updated_at: 1,
    };
    useAppStore.getState().setStreamMeta("session-1", {
      activeMessageId: "message-1",
      isStreaming: true,
    });

    renderHook(() => useChatSession("session-1", 1));

    expect(mocks.activeTurnQuery).toHaveBeenLastCalledWith("session-1", false);
    expect(mocks.liveRecoveryQuery).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      false,
    );
  });

  it("does not replay the timeline for a completed historical session", () => {
    mocks.activeTurn = {
      session_id: "session-1",
      turn_id: "turn-1",
      message_id: "message-1",
      status: "completed",
      last_event_id: "event-1",
      pending_user_action: null,
      updated_at: 1,
    };

    renderHook(() => useChatSession("session-1", 1));

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      false,
      expect.any(Function),
    );
  });

  it("refetches authoritative state when the timeline reports a terminal turn", async () => {
    renderHook(() => useChatSession("session-1", 1));
    const onTerminalTurn = mocks.timeline.mock.lastCall?.[3] as
      | (() => Promise<void>)
      | undefined;

    expect(onTerminalTurn).toBeDefined();
    await act(async () => onTerminalTurn?.());

    expect(mocks.sessionHistoryRefetch).toHaveBeenCalledTimes(1);
    expect(mocks.activeTurnRefetch).toHaveBeenCalledTimes(1);
  });

  it("allows an attachment-only turn and forwards ordered file ids", async () => {
    const { result } = renderHook(() => useChatSession("session-1", 0));

    await act(() => result.current.sendTurn({ fileIds: ["file-2", "file-1"] }));

    expect(mocks.createInitialTurnRequest).toHaveBeenCalledWith(
      "session-1",
      expect.any(String),
      "",
      undefined,
      undefined,
      [],
      ["file-2", "file-1"],
      undefined,
    );
    expect(mocks.send).toHaveBeenCalledTimes(1);
  });
});
