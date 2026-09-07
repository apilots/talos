// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAppStore } from "@/shared/store/app-store";
import { useSessionStreaming } from "./useSessionStreaming";

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
  cancelActiveTurn: vi.fn(),
  startTimelineTurn: vi.fn(),
  retryTimelineTurn: vi.fn(),
  invalidateSessionQueries: vi.fn(),
}));

vi.mock("../api", () => ({
  cancelActiveTurn: mocks.cancelActiveTurn,
  startTimelineTurn: mocks.startTimelineTurn,
  retryTimelineTurn: mocks.retryTimelineTurn,
  isActiveTurnRunningError: () => false,
}));

vi.mock("./useSessionQueryInvalidation", () => ({
  useSessionQueryInvalidation: () => ({
    invalidateSessionQueries: mocks.invalidateSessionQueries,
  }),
}));

describe("useSessionStreaming", () => {
  beforeEach(() => {
    useAppStore.setState({ streams: {}, transientStatuses: {} });
    mocks.startTimelineTurn.mockReset();
    mocks.retryTimelineTurn.mockReset().mockImplementation(async (_request, onAccepted) => {
      onAccepted("retry-message");
      return "retry-message";
    });
    mocks.cancelActiveTurn.mockReset().mockResolvedValue(undefined);
    mocks.invalidateSessionQueries.mockReset().mockResolvedValue(undefined);
  });

  it("shows a start error and clears the pending response state", async () => {
    mocks.startTimelineTurn.mockRejectedValue(
      new Error("Storage error: no such column: latest_turn_seq"),
    );
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    const { result } = renderHook(() => useSessionStreaming("session-1"), { wrapper });

    await act(async () => {
      await result.current.sendText("hello").catch(() => undefined);
    });

    expect(useAppStore.getState().streams["session-1"]).toMatchObject({
      connectionState: "idle",
      error: { code: "internalError", localization: null, message: "Storage error: no such column: latest_turn_seq" },
      isStreaming: false,
      pendingInput: undefined,
    });
    expect(useAppStore.getState().transientStatuses["session-1"]).toBe("error");
  });

  it("leaves the automatically dispatched next Turn owned by its events", async () => {
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    mocks.startTimelineTurn.mockImplementation(async () => {
      useAppStore.getState().setStreamMeta("session-1", {
        activeTurnId: "queued-turn", activeMessageId: "queued-message",
        isStreaming: true, connectionState: "streaming",
      });
    });
    const { result } = renderHook(() => useSessionStreaming("session-1"), { wrapper });
    await act(async () => { await result.current.sendText("first question"); });
    expect(useAppStore.getState().streams["session-1"]).toMatchObject({
      activeTurnId: "queued-turn", activeMessageId: "queued-message",
      isStreaming: true, connectionState: "streaming",
    });
  });

  it("keeps cancelling while the terminal refresh is pending", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    const { result } = renderHook(() => useSessionStreaming("session-1"), { wrapper });

    await act(async () => {
      await result.current.stopResponse("turn-1");
    });

    expect(useAppStore.getState().streams["session-1"]).toMatchObject({
      connectionState: "cancelling",
      isStreaming: true,
    });
    expect(mocks.invalidateSessionQueries).toHaveBeenCalledOnce();
  });

  it("accepts an attachment-only retry before completion and keeps real cancellation available", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    useAppStore.getState().setStreamMeta("session-1", {
      lastEventId: "ev_20", lastEventSeq: 20,
      activeMessageId: "message-1",
      activeClientMessageId: "client-1",
      activeInput: { text: "", fileIds: ["file-2", "file-1"] },
    });
    const { result } = renderHook(() => useSessionStreaming("session-1"), { wrapper });
    await act(async () => { await result.current.retryResponse(); });
    expect(useAppStore.getState().streams["session-1"]).toMatchObject({
      lastEventId: "ev_20", lastEventSeq: 20,
      isStreaming: true,
      activeMessageId: "retry-message",
      pendingInput: { text: "", fileIds: ["file-2", "file-1"], phase: "accepted", serverMessageId: "retry-message" },
    });
    expect(mocks.retryTimelineTurn).toHaveBeenCalledWith({
      sessionId: "session-1", clientMessageId: "client-1", retryOfMessageId: "message-1",
      input: { content: [{ type: "file", fileId: "file-2" }, { type: "file", fileId: "file-1" }] },
    }, expect.any(Function));
    expect(result.current.isCancelling).toBe(false);
    await act(() => result.current.stopResponse("retry-turn"));
    expect(mocks.cancelActiveTurn).toHaveBeenCalledWith("session-1", "retry-turn");
  });

  it("resolves send at admission while retaining the accepted live input", async () => {
    const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    mocks.startTimelineTurn.mockImplementation(async (_request, onAccepted) => {
      onAccepted("message-1");
      return "message-1";
    });
    const { result } = renderHook(() => useSessionStreaming("session-1"), { wrapper });
    await act(() => result.current.sendText("hello"));
    expect(useAppStore.getState().streams["session-1"]).toMatchObject({
      isStreaming: true, activeMessageId: "message-1",
      pendingInput: { text: "hello", phase: "accepted", serverMessageId: "message-1" },
    });
  });
});
