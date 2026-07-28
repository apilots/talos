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
  eventMessage: vi.fn(),
  invalidateSessionQueries: vi.fn(),
}));

vi.mock("./useSessionMessageStream", () => ({
  useSessionMessageStream: () => ({
    eventMessage: mocks.eventMessage,
    cancelStream: vi.fn(),
    retryMessage: vi.fn(),
  }),
}));

vi.mock("./useSessionQueryInvalidation", () => ({
  useSessionQueryInvalidation: () => ({
    invalidateSessionQueries: mocks.invalidateSessionQueries,
    refreshSessionTitleAfterTurn: vi.fn(),
  }),
}));

describe("useSessionStreaming", () => {
  beforeEach(() => {
    useAppStore.setState({ streams: {}, transientStatuses: {} });
    mocks.eventMessage.mockReset();
    mocks.invalidateSessionQueries.mockReset().mockResolvedValue(undefined);
  });

  it("shows a start error and clears the pending response state", async () => {
    mocks.eventMessage.mockRejectedValue(
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
      error: "Storage error: no such column: latest_turn_seq",
      isStreaming: false,
      pendingInput: undefined,
    });
    expect(useAppStore.getState().transientStatuses["session-1"]).toBe("error");
  });
});
