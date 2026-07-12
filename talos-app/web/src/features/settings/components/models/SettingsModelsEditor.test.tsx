// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

  it("offers provider-specific protocols for DeepSeek and OpenRouter", async () => {
    const provider = (protocol?: "chat" | "responses" | "messages") => ({
      api_key: "",
      base_url: "https://example.com",
      protocol,
      models: [],
    });
    getLlmConfig.mockResolvedValue({
      config: {
        provider: "deepseek",
        model: "deepseek-v4-pro",
        anthropic: provider("messages"),
        openai: provider("chat"),
        bailian: provider("chat"),
        deepseek: provider(),
        ollama: provider("chat"),
        openrouter: provider("chat"),
      },
      model_readiness: {
        ready: true,
        provider: "deepseek",
        model: "deepseek-v4-pro",
        issues: [],
      },
      provider_keys: {},
      provider_capabilities: {
        anthropic: { default_protocol: "messages", protocols: ["messages"] },
        openai: { default_protocol: "chat", protocols: ["chat", "responses"] },
        bailian: { default_protocol: "chat", protocols: ["chat"] },
        deepseek: { default_protocol: "messages", protocols: ["messages", "chat"] },
        ollama: { default_protocol: "chat", protocols: ["chat"] },
        openrouter: {
          default_protocol: "chat",
          protocols: ["messages", "responses", "chat"],
        },
      },
    });

    render(<SettingsModelsEditor />, { wrapper: createWrapper() });

    const deepseekTab = await screen.findByRole("button", { name: "DeepSeek" });
    fireEvent.click(deepseekTab);
    let protocolSelect = screen.getByDisplayValue("Messages");
    expect(within(protocolSelect).getAllByRole("option").map((option) => option.textContent)).toEqual([
      "Messages",
      "Chat",
    ]);

    fireEvent.click(screen.getByRole("button", { name: "OpenRouter" }));
    protocolSelect = screen.getByDisplayValue("Chat");
    expect(within(protocolSelect).getAllByRole("option").map((option) => option.textContent)).toEqual([
      "Messages",
      "Responses",
      "Chat",
    ]);
  });
});
