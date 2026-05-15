// @vitest-environment jsdom

import { createElement, type ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createJSONStorage } from "zustand/middleware";

import { useSessionApproval } from "./useSessionApproval";
import { submitToolApproval } from "../api";
import { useAppStore } from "@/shared/store/app-store";

vi.mock("../api", () => ({
  getSession: vi.fn(),
  submitToolApproval: vi.fn(),
}));

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("useSessionApproval", () => {
  let storage: Map<string, string>;

  beforeEach(() => {
    storage = new Map<string, string>();
    useAppStore.persist.setOptions({
      storage: createJSONStorage(() => ({
        getItem: (key: string) => storage.get(key) ?? null,
        setItem: (key: string, value: string) => {
          storage.set(key, value);
        },
        removeItem: (key: string) => {
          storage.delete(key);
        },
      })),
    });
    vi.mocked(submitToolApproval).mockReset();
    useAppStore.getState().resetStream("sess-approval");
  });

  it("submits approval without creating an empty continuation message", async () => {
    vi.mocked(submitToolApproval).mockResolvedValue(undefined);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    useAppStore.getState().setPendingApproval("sess-approval", {
      callId: "call-1",
      toolName: "Bash",
      prompt: "Approve Bash?",
      arguments: { command: "pwd" },
    });
    useAppStore.getState().setStreamMeta("sess-approval", {
      isStreaming: true,
      connectionState: "detached",
      awaitingFinalResponse: true,
      showReconnectControls: true,
    });
    useAppStore.getState().setSessionStatus("sess-approval", "waiting_approval");

    const { result } = renderHook(() => useSessionApproval("sess-approval"), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await result.current.submitApproval(true);
    });

    expect(submitToolApproval).toHaveBeenCalledWith("sess-approval", {
      call_id: "call-1",
      tool_name: "Bash",
      approved: true,
    });
    const stream = useAppStore.getState().streams["sess-approval"];
    expect(stream).toMatchObject({
      pendingApproval: undefined,
      pendingUserAction: undefined,
      isStreaming: false,
      connectionState: "idle",
      awaitingFinalResponse: false,
      showReconnectControls: false,
    });
    expect(useAppStore.getState().transientStatuses["sess-approval"]).toBeUndefined();
    expect(invalidateSpy).toHaveBeenCalled();
  });
});
