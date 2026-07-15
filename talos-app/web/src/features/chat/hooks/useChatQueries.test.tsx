// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ModelSelection, SessionInfo } from "@talos/sdk-browser";

import { chatKeys, useUpdateSessionModelMutation } from "./useChatQueries";

const apiMocks = vi.hoisted(() => ({
  updateSessionModel: vi.fn(),
}));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  updateSessionModel: apiMocks.updateSessionModel,
}));

vi.mock("@/shared/store/app-store", () => ({
  useAppStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector({ connection: { authToken: "token", userId: "user-1" } }),
}));

describe("useUpdateSessionModelMutation", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("updates the cached model and reasoning effort before the request completes", async () => {
    let resolveUpdate: (() => void) | undefined;
    apiMocks.updateSessionModel.mockReturnValue(
      new Promise<void>((resolve) => {
        resolveUpdate = resolve;
      }),
    );
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const connectionKey = "token|user-1";
    const sessionKey = chatKeys.session(connectionKey, "session-1");
    queryClient.setQueryData(sessionKey, {
      modelSelection: {
        modelRef: { provider: "deepseek", id: "deepseek-v4-flash" },
        reasoningEffort: "light",
      },
    } as SessionInfo);
    const wrapper = ({ children }: PropsWithChildren) =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    const { result } = renderHook(() => useUpdateSessionModelMutation("session-1"), { wrapper });
    const nextSelection: ModelSelection = {
      modelRef: { provider: "anthropic", id: "claude-opus" },
      reasoningEffort: "high",
    };

    act(() => result.current.mutate(nextSelection));

    await waitFor(() => {
      expect(queryClient.getQueryData<SessionInfo>(sessionKey)?.modelSelection).toEqual(
        nextSelection,
      );
    });

    resolveUpdate?.();
  });
});
