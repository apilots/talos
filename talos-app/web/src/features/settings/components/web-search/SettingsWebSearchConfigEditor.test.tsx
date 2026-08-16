// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SettingsWebSearchConfigEditor } from "./SettingsWebSearchConfigEditor";

const getWebSearchConfig = vi.fn();
const updateWebSearchConfig = vi.fn();

vi.mock("@/features/settings/api", () => ({
  getWebSearchConfig: () => getWebSearchConfig(),
  updateWebSearchConfig: (config: unknown) => updateWebSearchConfig(config),
}));

vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "settings-web-search-test",
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

function webSearchResponse() {
  return {
    type: "webSearch",
    config: {
      provider: "firecrawl",
      firecrawl: {
        api_key: "********",
        base_url: "https://api.firecrawl.dev",
        timeout_ms: 15000,
      },
      brave: {
        api_key: undefined,
        base_url: "https://api.search.brave.com/res/v1/web/search",
        timeout_ms: 15000,
      },
      tavily: {
        api_key: undefined,
        base_url: "https://api.tavily.com",
        timeout_ms: 15000,
      },
    },
  };
}

describe("SettingsWebSearchConfigEditor", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("edits provider-specific web search config", async () => {
    getWebSearchConfig.mockResolvedValue(webSearchResponse());
    updateWebSearchConfig.mockResolvedValue({
      ...webSearchResponse(),
      config: {
        ...webSearchResponse().config,
        provider: "brave",
      },
    });

    render(<SettingsWebSearchConfigEditor />, { wrapper: createWrapper() });

    fireEvent.click(await screen.findByRole("button", { name: "Brave" }));
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "brave-secret" },
    });
    fireEvent.change(screen.getByDisplayValue("https://api.search.brave.com/res/v1/web/search"), {
      target: { value: "https://brave.example/search" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));

    await waitFor(() => expect(updateWebSearchConfig).toHaveBeenCalledTimes(1));
    expect(updateWebSearchConfig.mock.calls[0][0].brave.api_key).toBe("brave-secret");
    expect(updateWebSearchConfig.mock.calls[0][0].brave.base_url).toBe("https://brave.example/search");
  });
});
