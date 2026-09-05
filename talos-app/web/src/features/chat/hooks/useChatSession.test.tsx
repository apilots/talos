// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";

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
    sessionId: "session-1",
    input: { content: [{ type: "file", fileId: "file-1" }] },
  })),
  send: vi.fn(),
  sessionHistoryRefetch: vi.fn().mockResolvedValue(undefined),
  messageQueueStart: vi.fn(),
  messageQueueHook: vi.fn(),
  timeline: vi.fn(),
}));

vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api")>();
  return {
    ...actual,
    createInitialTurnRequest: mocks.createInitialTurnRequest,
  };
});

vi.mock("./useChatQueries", () => ({
  useActiveTurnQuery: (...args: unknown[]) => {
    mocks.activeTurnQuery(...args);
    return {
      data: mocks.activeTurn,
      error: undefined,
      refetch: mocks.activeTurnRefetch,
    };
  },
  useSessionHistoryQuery: (_sessionId: string, enabled: boolean) => ({
    data: { data: [], nextCursor: null },
    error: undefined,
    isSuccess: enabled,
    refetch: mocks.sessionHistoryRefetch,
  }),
}));

vi.mock("./useSessionBlocks", () => ({
  useSessionBlocks: () => [],
}));

vi.mock("./useSessionQueryInvalidation", () => ({
  useSessionQueryInvalidation: () => ({
    invalidateSessionQueries: async () => {
      await mocks.sessionHistoryRefetch();
      await mocks.activeTurnRefetch();
    },
  }),
}));

vi.mock("./useSessionMessageQueue", () => ({
  useSessionMessageQueue: (...args: unknown[]) => {
    mocks.messageQueueHook(...args);
    return {
    queue: undefined,
    error: undefined,
    add: vi.fn(),
    update: vi.fn(),
    delete: vi.fn(),
    reorder: vi.fn(),
    steer: vi.fn(),
    start: mocks.messageQueueStart,
    };
  },
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
    mocks.send.mockReset().mockRejectedValue(new TalosApiError({
      code: AppProtocol.ErrorCode.ActiveTurnRunning,
      message: "Active turn is already running",
    }));
    mocks.createInitialTurnRequest.mockClear();
    mocks.activeTurnRefetch.mockClear();
    mocks.activeTurnQuery.mockClear();
    mocks.sessionHistoryRefetch.mockClear();
    mocks.messageQueueStart.mockReset().mockResolvedValue(undefined);
    mocks.messageQueueHook.mockClear();
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
      expect.objectContaining({ start: mocks.messageQueueStart }),
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
      expect.objectContaining({ start: mocks.messageQueueStart }),
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
      expect.objectContaining({ start: mocks.messageQueueStart }),
    );
  });

  it("observes idle-session maintenance before a manual command starts", () => {
    const { result } = renderHook(() => useChatSession("session-1", 0));

    act(() => result.current.setCompactionObservationActive(true));

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      true,
      expect.any(Function),
      expect.objectContaining({ start: mocks.messageQueueStart }),
    );
  });

  it("does not transfer idle maintenance observation to another session", () => {
    let sessionId = "session-1";
    const { result, rerender } = renderHook(() => useChatSession(sessionId));

    act(() => result.current.setCompactionObservationActive(true));
    sessionId = "session-2";
    rerender();

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-2",
      undefined,
      false,
      expect.any(Function),
      expect.objectContaining({ start: mocks.messageQueueStart }),
    );
  });

  it("does not poll active turn snapshots while a local event stream is active", () => {
    mocks.activeTurn = {
      identity: { sessionId: "session-1", turnId: "turn-1" },
      initialMessageId: "message-1",
      state: "waitingForInput",
      lastEventId: "event-1",
      updatedAt: 1,
    };
    useAppStore.getState().setStreamMeta("session-1", {
      activeMessageId: "message-1",
      isStreaming: true,
    });

    renderHook(() => useChatSession("session-1", 1));

    expect(mocks.activeTurnQuery).toHaveBeenLastCalledWith("session-1", false);
  });

  it("keeps session events subscribed across idle and active turns", () => {
    mocks.activeTurn = undefined;

    renderHook(() => useChatSession("session-1", 1));

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      true,
      expect.any(Function),
      expect.objectContaining({ start: mocks.messageQueueStart }),
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
      "",
      {
        fileIds: ["file-2", "file-1"],
        modelSelection: undefined,
        project: undefined,
        userPermissionMode: undefined,
      },
    );
    expect(mocks.send).toHaveBeenCalledTimes(1);
  });
});
