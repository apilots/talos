// @vitest-environment jsdom

import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createJSONStorage } from "zustand/middleware";

import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { useSettingsPage } from "./useSettingsPage";

vi.mock("@/features/chat", () => ({
  useModelsQuery: () => ({
    data: [
      { id: "glm-5", name: "GLM-5", provider: "zhipu", context_window: 128000, max_tokens: 8192 },
      { id: "gpt-4.1", name: "GPT-4.1", provider: "openai", context_window: 128000, max_tokens: 8192 },
    ],
  }),
}));

describe("useSettingsPage", () => {
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
      connection: {
        ...state.connection,
        serverUrl: "http://localhost:8080",
        authToken: "",
        userId: "web-user",
        runtimeMode: "cloud",
        defaultApprovalMode: "needs_approval",
      },
      locale: "en",
      uiTheme: "light",
    }));
  });

  it("hydrates the default model from the first available model", async () => {
    const { result } = renderHook(() => useSettingsPage());

    await waitFor(() => {
      expect(result.current.defaultModel).toBe("glm-5");
    });
  });

  it("tracks connection changes into the local draft", async () => {
    const { result, rerender } = renderHook(() => useSettingsPage());

    useAppStore.getState().setConnection({ serverUrl: "http://127.0.0.1:9000", userId: "alice" });
    rerender();

    await waitFor(() => {
      expect(result.current.draft.serverUrl).toBe("http://127.0.0.1:9000");
      expect(result.current.draft.userId).toBe("alice");
    });
  });
});
