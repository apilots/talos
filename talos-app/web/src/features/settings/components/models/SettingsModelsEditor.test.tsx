// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SettingsModelsEditor } from "./SettingsModelsEditor";

const getLlmConfig = vi.fn();

vi.mock("@/features/settings/api", () => ({
  getLlmConfig: () => getLlmConfig(),
  updateLlmConfig: vi.fn(),
}));

vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "settings-models-test",
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return function Wrapper({ children }: PropsWithChildren) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("SettingsModelsEditor", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("shows config read errors instead of staying in loading state", async () => {
    getLlmConfig.mockRejectedValue(new Error("failed to read config"));

    render(<SettingsModelsEditor />, { wrapper: createWrapper() });

    await waitFor(() => {
      expect(screen.getByText("failed to read config")).toBeTruthy();
    });
    expect(screen.queryByText(/Loading configuration|加载配置中/)).toBeNull();
  });
});
