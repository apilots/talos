// @vitest-environment jsdom

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createJSONStorage } from "zustand/middleware";
import type { PropsWithChildren } from "react";

import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { adminSections, useAdminPage } from "./useAdminPage";

const listAdminSessionsMock = vi.fn((query?: { limit?: number; offset?: number }) =>
  Promise.resolve({
    data: [
      {
        title: "Review frontend architecture",
        user_id: "owner-1",
        config: { llm: { model: "glm-5" } },
        context: { session_id: "sess-1", user_id: "source-1" },
        state: { last_activity: Date.now() - 1_000, created_at: Date.now() - 10_000 },
      },
      {
        title: "Fix admin layout",
        user_id: "owner-2",
        config: { llm: { model: "gpt-4.1" } },
        context: { session_id: "sess-2", user_id: "source-2" },
        state: { last_activity: Date.now() - 2_000, created_at: Date.now() - 20_000 },
      },
    ],
    pagination: {
      total: 42,
      limit: query?.limit ?? 20,
      offset: query?.offset ?? 0,
    },
  }),
);

vi.mock("@/features/chat", () => ({
  listAdminSessions: (query?: { limit?: number; offset?: number }) => listAdminSessionsMock(query),
  useConnectionKey: () => "test-connection",
  useModelsQuery: () => ({
    data: [{ id: "glm-5", name: "GLM-5", provider: "zhipu", context_window: 128000 }],
  }),
}));

vi.mock("@/features/auth", () => ({
  listUsers: () =>
    Promise.resolve([
      { user_id: "owner-1", username: "xujiang" },
      { user_id: "owner-2", username: "admin" },
    ]),
}));

vi.mock("@/features/connection", () => ({
  useConnectionSettings: () => ({
    connection: {
      serverUrl: "http://127.0.0.1:8080",
      authToken: "",
      userId: "web-user",
      runtimeMode: "cloud",
    },
  }),
}));

describe("useAdminPage", () => {
  function createWrapper() {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    });

    return function Wrapper({ children }: PropsWithChildren) {
      return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
    };
  }

  beforeEach(() => {
    listAdminSessionsMock.mockClear();
    const storage = new Map<string, string>();
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
    useAppStore.setState((state) => ({
      ...state,
      adminMetrics: [],
    }));
  });

  it("derives localized metrics from the current queries", async () => {
    const { result } = renderHook(() => useAdminPage(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(result.current.metrics).toHaveLength(3);
      expect(result.current.metrics[0]?.value).toBe("42");
      expect(result.current.metrics[1]?.value).toBe("1");
    });
  });

  it("keeps model configuration out of the admin section list", () => {
    expect(adminSections).not.toContain("models");
    expect(adminSections).toContain("system");
  });

  it("filters session rows by search query", async () => {
    const { result } = renderHook(() => useAdminPage(), { wrapper: createWrapper() });

    result.current.setSessionSearch("frontend");

    await waitFor(() => {
      expect(result.current.sessionRows).toHaveLength(1);
      expect(result.current.sessionRows[0]?.session.context.session_id).toBe("sess-1");
    });
  });

  it("requests the selected admin sessions page from the backend", async () => {
    const { result } = renderHook(() => useAdminPage(), { wrapper: createWrapper() });

    await waitFor(() => {
      expect(listAdminSessionsMock).toHaveBeenCalledWith({ limit: 20, offset: 0 });
    });

    act(() => {
      result.current.setSessionPage(2);
    });

    await waitFor(() => {
      expect(listAdminSessionsMock).toHaveBeenCalledWith({ limit: 20, offset: 20 });
      expect(result.current.sessionPagination.page).toBe(2);
    });
  });
});
