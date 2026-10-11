// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ModelSelection, SessionInfo } from "@talos/sdk-browser";

import { connectionScope } from "@/shared/store/connection-scope";
import { useAppStore } from "@/shared/store/app-store";
import { chatKeys, useSessionActions, useUpdateSessionModelMutation } from "./useChatQueries";

const apiMocks = vi.hoisted(() => ({
  compactSession: vi.fn(),
  updateSessionModel: vi.fn(),
}));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  compactSession: apiMocks.compactSession,
  updateSessionModel: apiMocks.updateSessionModel,
}));

vi.mock("@/shared/store/app-store", () => ({
  useAppStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector({ connection: { authToken: "token", userId: "user-1" } }),
}));

function createQueryHarness() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: PropsWithChildren) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return { queryClient, wrapper };
}

describe("useUpdateSessionModelMutation", () => {
  afterEach(() => {
    cleanup();
    apiMocks.updateSessionModel.mockReset();
    vi.restoreAllMocks();
  });

  it("updates the cached model and reasoning effort before the request completes", async () => {
    let resolveUpdate: (() => void) | undefined;
    apiMocks.updateSessionModel.mockReturnValue(
      new Promise<void>((resolve) => {
        resolveUpdate = resolve;
      }),
    );
    const { queryClient, wrapper } = createQueryHarness();
    const connectionKey = useAppStore((state) => connectionScope(state.connection));
    const sessionKey = chatKeys.session(connectionKey, "session-1");
    queryClient.setQueryData(sessionKey, {
      modelSelection: {
        modelRef: { provider: "deepseek", id: "deepseek-v4-flash" },
        reasoningEffort: "light",
      },
    } as SessionInfo);
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

  it("persists consecutive selections in the order they were made", async () => {
    const resolvers: Array<() => void> = [];
    apiMocks.updateSessionModel.mockImplementation(
      () => new Promise<void>((resolve) => resolvers.push(resolve)),
    );
    const { queryClient, wrapper } = createQueryHarness();
    const sessionKey = chatKeys.session(useAppStore((state) => connectionScope(state.connection)), "session-1");
    queryClient.setQueryData(sessionKey, {
      modelSelection: {
        modelRef: { provider: "deepseek", id: "deepseek-v4-flash" },
        reasoningEffort: "light",
      },
    } as SessionInfo);
    const { result } = renderHook(() => useUpdateSessionModelMutation("session-1"), { wrapper });
    const firstSelection: ModelSelection = {
      modelRef: { provider: "anthropic", id: "claude-opus" },
      reasoningEffort: "medium",
    };
    const finalSelection: ModelSelection = {
      modelRef: { provider: "anthropic", id: "claude-opus" },
      reasoningEffort: "high",
    };

    act(() => {
      result.current.mutate(firstSelection);
      result.current.mutate(finalSelection);
    });

    await waitFor(() => expect(apiMocks.updateSessionModel).toHaveBeenCalledTimes(1));
    expect(apiMocks.updateSessionModel).toHaveBeenNthCalledWith(
      1,
      "session-1",
      firstSelection,
    );
    expect(queryClient.getQueryData<SessionInfo>(sessionKey)?.modelSelection).toEqual(
      finalSelection,
    );

    act(() => resolvers[0]?.());

    await waitFor(() => expect(apiMocks.updateSessionModel).toHaveBeenCalledTimes(2));
    expect(apiMocks.updateSessionModel).toHaveBeenNthCalledWith(
      2,
      "session-1",
      finalSelection,
    );
    expect(queryClient.getQueryData<SessionInfo>(sessionKey)?.modelSelection).toEqual(
      finalSelection,
    );

    act(() => resolvers[1]?.());
    await waitFor(() => {
      expect(queryClient.getQueryData<SessionInfo>(sessionKey)?.modelSelection).toEqual(
        finalSelection,
      );
    });
  });
});

describe("useSessionActions", () => {
  afterEach(() => {
    cleanup();
    apiMocks.compactSession.mockReset();
    vi.restoreAllMocks();
  });

  it("stores the compacted session as the current session cache", async () => {
    const previousSession = {
      id: "session-1",
      usage: {
        tokens: {
          input_tokens: 11_037,
          output_tokens: 35,
          total_tokens: 11_072,
          cache_creation_input_tokens: null,
          cache_read_input_tokens: 3_712,
        },
        context: {
          contextWindowTokens: 258_000,
          effectivePromptBudgetTokens: 250_000,
          measuredAt: 1,
          usedTokens: 219_000,
        },
      },
    } as SessionInfo;
    const compactedSession = {
      ...previousSession,
      usage: {
        tokens: {
          input_tokens: 11_341,
          output_tokens: 452,
          total_tokens: 11_793,
          cache_creation_input_tokens: null,
          cache_read_input_tokens: 3_968,
        },
        context: previousSession.usage.context,
      },
    } as SessionInfo;
    apiMocks.compactSession.mockResolvedValue(compactedSession);
    const { queryClient, wrapper } = createQueryHarness();
    const sessionKey = chatKeys.session(useAppStore((state) => connectionScope(state.connection)), "session-1");
    queryClient.setQueryData(sessionKey, previousSession);
    const { result } = renderHook(() => useSessionActions("session-1"), { wrapper });

    await act(async () => result.current.compact.mutateAsync());

    expect(queryClient.getQueryData<SessionInfo>(sessionKey)).toEqual(compactedSession);
  });
});
