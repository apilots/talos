// @vitest-environment jsdom

import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createJSONStorage } from "zustand/middleware";

import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { useAdminPage } from "./useAdminPage";

vi.mock("@/features/chat", () => ({
  useModelsQuery: () => ({
    data: [{ id: "glm-5", name: "GLM-5", provider: "zhipu", context_window: 128000, max_tokens: 8192 }],
  }),
  useSessionsQuery: () => ({
    data: [
      {
        title: "Review frontend architecture",
        config: { llm: { model: "glm-5" } },
        context: { session_id: "sess-1" },
        state: { last_activity: Date.now() - 1_000, created_at: Date.now() - 10_000 },
      },
      {
        title: "Fix admin layout",
        config: { llm: { model: "gpt-4.1" } },
        context: { session_id: "sess-2" },
        state: { last_activity: Date.now() - 2_000, created_at: Date.now() - 20_000 },
      },
    ],
  }),
}));

vi.mock("@/features/connection", () => ({
  useConnectionSettings: () => ({
    connection: {
      serverUrl: "http://127.0.0.1:8080",
      authToken: "",
      userId: "web-user",
      runtimeMode: "cloud",
      defaultApprovalMode: "needs_approval",
    },
  }),
}));

describe("useAdminPage", () => {
  beforeEach(() => {
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
    const { result } = renderHook(() => useAdminPage());

    await waitFor(() => {
      expect(result.current.metrics).toHaveLength(4);
      expect(result.current.metrics[0]?.value).toBe("2");
      expect(result.current.metrics[1]?.value).toBe("1");
    });
  });

  it("filters session rows by search query", async () => {
    const { result } = renderHook(() => useAdminPage());

    result.current.setSessionSearch("frontend");

    await waitFor(() => {
      expect(result.current.sessionRows).toHaveLength(1);
      expect(result.current.sessionRows[0]?.context.session_id).toBe("sess-1");
    });
  });
});
