// @vitest-environment jsdom

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useSessionMessageStream } from "./useSessionMessageStream";
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

const apiMocks = vi.hoisted(() => ({
  retryTimelineTurn: vi.fn(),
  startTimelineTurn: vi.fn().mockResolvedValue("message-1"),
}));

vi.mock("../api", () => apiMocks);

describe("useSessionMessageStream", () => {
  beforeEach(() => {
    apiMocks.startTimelineTurn.mockClear();
    useAppStore.getState().resetStream("session-1");
  });

  it("waits for the session event stream instead of opening a message subscription", async () => {
    useAppStore.getState().setStreamMeta("session-1", {
      activeMessageId: "message-1",
      isStreaming: true,
    });
    const { result } = renderHook(() => useSessionMessageStream("session-1"));
    let completion: Promise<void> | undefined;
    act(() => {
      completion = result.current.eventMessage({
        session_id: "session-1",
        input: [{ type: "text", text: "hello" }],
      });
    });
    await waitFor(() => expect(apiMocks.startTimelineTurn).toHaveBeenCalledTimes(1));

    act(() => {
      useAppStore.getState().setStreamMeta("session-1", { isStreaming: false });
    });
    await expect(completion).resolves.toBeUndefined();
  });
});
