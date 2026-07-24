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
    apiMocks.retryTimelineTurn.mockReset();
    apiMocks.retryTimelineTurn.mockResolvedValue("message-2");
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

  it("retries attachment-only input with the same ordered file ids", async () => {
    useAppStore.getState().setStreamMeta("session-1", {
      activeMessageId: "message-2",
      isStreaming: true,
    });
    const { result } = renderHook(() => useSessionMessageStream("session-1"));
    let completion: Promise<void> | undefined;
    act(() => {
      completion = result.current.retryMessage({
        clientMessageId: "client-1",
        retryOfMessageId: "message-1",
        input: { text: "", fileIds: ["file-2", "file-1"] },
      });
    });
    await waitFor(() => expect(apiMocks.retryTimelineTurn).toHaveBeenCalledTimes(1));

    expect(apiMocks.retryTimelineTurn).toHaveBeenCalledWith(
      {
        session_id: "session-1",
        client_message_id: "client-1",
        retry_of_message_id: "message-1",
        input: [{ type: "files", fileIds: ["file-2", "file-1"] }],
      },
      undefined,
    );

    act(() => {
      useAppStore.getState().setStreamMeta("session-1", { isStreaming: false });
    });
    await expect(completion).resolves.toBeUndefined();
  });
});
