// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { SessionInfo, SessionListEntry } from "@talos/sdk-browser";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { SessionDetail } from "@/shared/api/product-queries";
import type { ConnectionState } from "@/features/connection/store";
import { connectionScope } from "@/shared/store/connection-scope";
import { chatKeys } from "./useChatQueries";
import { useToggleSessionPinMutation } from "./useToggleSessionPinMutation";

const apiMocks = vi.hoisted(() => ({
  updateSessionPinned: vi.fn<typeof import("../api")["updateSessionPinned"]>(),
  connection: {
    authToken: "token",
    authStatus: "authenticated",
    authRole: "user",
    accountType: "Pro",
    authUsername: "user",
    userId: "user-1",
  } satisfies ConnectionState,
}));

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  updateSessionPinned: apiMocks.updateSessionPinned,
}));

vi.mock("@/shared/store/app-store", () => ({
  useAppStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector({ connection: apiMocks.connection, selectedComputers: {} }),
}));

function makeSessionInfo(id: string, pinned: boolean): SessionInfo {
  return {
    id,
    title: `Session ${id}`,
    modelSelection: {
      modelRef: { provider: "mock", id: "model-1" },
      reasoningEffort: null,
    },
    createdAt: 1,
    lastActivity: 10,
    pinned,
  } as SessionInfo;
}

function makeListEntry(id: string, pinned: boolean, hasActiveTurn = false): SessionListEntry {
  return {
    ...makeSessionInfo(id, pinned),
    hasActiveTurn,
  } as SessionListEntry;
}

function createHarness() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const wrapper = ({ children }: PropsWithChildren) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  const connectionKey = connectionScope(apiMocks.connection);
  const sessionsKey = chatKeys.sessions(JSON.stringify(["local", "node-1"]));
  return { queryClient, wrapper, connectionKey, sessionsKey };
}

describe("useToggleSessionPinMutation", () => {
  afterEach(() => {
    cleanup();
    apiMocks.updateSessionPinned.mockReset();
  });

  it("syncs the sessions list cache on success while preserving entry fields and ignoring skipped invalidation", async () => {
    const resolvers = new Map<string, (session: SessionInfo) => void>();
    apiMocks.updateSessionPinned.mockImplementation(
      (sessionId) =>
        new Promise<SessionInfo>((resolve) => {
          resolvers.set(sessionId, resolve);
        }),
    );

    const { queryClient, wrapper, connectionKey, sessionsKey } = createHarness();
    const detailKeyA = chatKeys.session(connectionKey, "session-a");
    queryClient.setQueryData<SessionListEntry[]>(sessionsKey, [
      makeListEntry("session-a", false, true),
      makeListEntry("session-b", false, false),
    ]);
    queryClient.setQueryData<SessionDetail>(detailKeyA, {
      owner: "server",
      session: makeSessionInfo("session-a", false),
    } as SessionDetail);

    const { result } = renderHook(() => useToggleSessionPinMutation(), { wrapper });

    let promiseA!: Promise<SessionInfo>;
    let promiseB!: Promise<SessionInfo>;
    act(() => {
      promiseA = result.current.mutateAsync({ sessionId: "session-a", pinned: true });
      promiseB = result.current.mutateAsync({ sessionId: "session-b", pinned: true });
    });

    await waitFor(() => {
      expect(apiMocks.updateSessionPinned).toHaveBeenCalledTimes(2);
      const list = queryClient.getQueryData<SessionListEntry[]>(sessionsKey);
      expect(list?.find((entry) => entry.id === "session-a")?.pinned).toBe(true);
    });

    // Simulate a background poll landing before session-a settles.
    queryClient.setQueryData<SessionListEntry[]>(sessionsKey, [
      makeListEntry("session-a", false, true),
      makeListEntry("session-b", true, false),
    ]);

    // Resolve session-a while session-b is still in flight (isMutating > 1 skips onSettled invalidation).
    await act(async () => {
      resolvers.get("session-a")?.(makeSessionInfo("session-a", true));
      await promiseA;
    });

    const listAfterA = queryClient.getQueryData<SessionListEntry[]>(sessionsKey);
    const updatedA = listAfterA?.find((entry) => entry.id === "session-a");
    expect(updatedA?.pinned).toBe(true);
    expect(updatedA?.hasActiveTurn).toBe(true);
    expect(queryClient.getQueryData<SessionDetail>(detailKeyA)?.session.pinned).toBe(true);

    await act(async () => {
      resolvers.get("session-b")?.(makeSessionInfo("session-b", true));
      await promiseB;
    });
  });

  it("does not overwrite a newer in-flight toggle for the same session", async () => {
    const resolvers: Array<(session: SessionInfo) => void> = [];
    apiMocks.updateSessionPinned.mockImplementation(
      () =>
        new Promise<SessionInfo>((resolve) => {
          resolvers.push(resolve);
        }),
    );

    const { queryClient, wrapper, connectionKey, sessionsKey } = createHarness();
    const detailKeyA = chatKeys.session(connectionKey, "session-a");
    queryClient.setQueryData<SessionListEntry[]>(sessionsKey, [
      makeListEntry("session-a", false, false),
    ]);
    queryClient.setQueryData<SessionDetail>(detailKeyA, {
      owner: "server",
      session: makeSessionInfo("session-a", false),
    } as SessionDetail);

    const { result } = renderHook(() => useToggleSessionPinMutation(), { wrapper });

    let firstPromise!: Promise<SessionInfo>;
    let secondPromise!: Promise<SessionInfo>;
    act(() => {
      firstPromise = result.current.mutateAsync({ sessionId: "session-a", pinned: true });
      secondPromise = result.current.mutateAsync({ sessionId: "session-a", pinned: false });
    });

    await waitFor(() => {
      expect(apiMocks.updateSessionPinned).toHaveBeenCalledTimes(2);
      const list = queryClient.getQueryData<SessionListEntry[]>(sessionsKey);
      expect(list?.[0]?.pinned).toBe(false);
    });

    await act(async () => {
      resolvers[0]?.(makeSessionInfo("session-a", true));
      await firstPromise;
    });

    expect(queryClient.getQueryData<SessionListEntry[]>(sessionsKey)?.[0]?.pinned).toBe(false);
    expect(queryClient.getQueryData<SessionDetail>(detailKeyA)?.session.pinned).toBe(false);

    await act(async () => {
      resolvers[1]?.(makeSessionInfo("session-a", false));
      await secondPromise;
    });

    expect(queryClient.getQueryData<SessionListEntry[]>(sessionsKey)?.[0]?.pinned).toBe(false);
  });
});
