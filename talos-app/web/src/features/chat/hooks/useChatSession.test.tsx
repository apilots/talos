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
  send: vi.fn().mockRejectedValue({ code: -32013 }),
  timeline: vi.fn(),
}));

vi.mock("../api", () => ({
  createInitialTurnRequest: vi.fn(),
  isActiveTurnRunningError: (error: unknown) =>
    Boolean(error && typeof error === "object" && "code" in error && error.code === -32013),
}));

vi.mock("./useChatQueries", () => ({
  useActiveTurnQuery: () => ({ data: undefined, error: undefined }),
  useLiveRecoveryQuery: () => ({ data: undefined }),
  useSessionHistoryQuery: () => ({
    data: { last_stable_event_id: null, messages: [] },
    error: undefined,
    isSuccess: true,
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
    mocks.timeline.mockClear();
    useAppStore.getState().resetStream("session-1");
  });

  it("restores the draft when the server reports an active turn", async () => {
    const { result } = renderHook(() => useChatSession("session-1", 0));
    act(() => result.current.setDraft("keep this input"));

    await act(() => result.current.sendTurn());

    expect(mocks.send).toHaveBeenCalledTimes(1);
    expect(result.current.draft).toBe("keep this input");
  });

  it("starts the new session timeline after the first turn is accepted", () => {
    const { rerender } = renderHook(() => useChatSession("session-1"));

    expect(mocks.timeline).toHaveBeenLastCalledWith("session-1", undefined, false);

    act(() => {
      useAppStore.getState().setStreamMeta("session-1", {
        activeMessageId: "message-1",
      });
    });
    rerender();

    expect(mocks.timeline).toHaveBeenLastCalledWith("session-1", undefined, true);
  });
});
