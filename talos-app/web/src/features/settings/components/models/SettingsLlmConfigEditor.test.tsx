// @vitest-environment jsdom
import userEvent from "@testing-library/user-event";

import "@testing-library/jest-dom/vitest";
import { installRadixTestEnvironment, selectOption } from "./settingsTestUtils";

import { createElement, StrictMode, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { SettingsLlmConfigEditor } from "./SettingsLlmConfigEditor";
import { emptyModelForm, formToModel } from "./modelForm";

installRadixTestEnvironment();

const getLlmConfig = vi.fn();
const updateLlmConfig = vi.fn();

vi.mock("@/features/settings/api", () => ({
  getLlmConfig: () => getLlmConfig(),
  updateLlmConfig: (...args: unknown[]) => updateLlmConfig(...args),
}));

vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "settings-models-test",
  chatKeys: { models: (key: string) => ["talos", key, "models"] },
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

function customProviderFixture() {
  const connection = { api_key: "", base_url: "http://localhost:8000/v1", protocol: "chat" as const, models: [], http_timeouts: { connect_secs: 5, response_secs: 30, stream_idle_secs: 45, stream_total_secs: 120 } };
  const capabilities = { default_protocol: "chat", protocols: ["chat"] };
  return {
    config: {
      provider: "deepseek", model: "model",
      anthropic: { ...connection, protocol: "messages" as const }, openai: connection, bailian: connection,
      deepseek: { ...connection, models: [formToModel({ ...emptyModelForm, id: "model", name: "DeepSeek" })] },
      ollama: connection, openrouter: connection,
      custom_providers: {
        "llm-gateway": { ...connection, name: "LLM Gateway", models: [formToModel({ ...emptyModelForm, id: "model", name: "Qwen3 32B" })] },
      },
    },
    provider_keys: {},
    provider_capabilities: { anthropic: { default_protocol: "messages", protocols: ["messages"] }, openai: capabilities, deepseek: capabilities },
  };
}

async function showCustomProvider() {
  render(<StrictMode><SettingsLlmConfigEditor /></StrictMode>, { wrapper: createWrapper() });
  await userEvent.click(await screen.findByRole("button", { name: "Custom" }));
  await userEvent.click(await screen.findByRole("menuitem", { name: "LLM Gateway" }));
}

describe("SettingsLlmConfigEditor", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("saves a new custom provider without randomUUID on an HTTP origin", async () => {
    vi.stubGlobal("crypto", { getRandomValues: crypto.getRandomValues.bind(crypto) });
    const response = customProviderFixture();
    getLlmConfig.mockResolvedValue(response);
    updateLlmConfig.mockImplementation(async (config) => ({ ...response, config }));
    render(<SettingsLlmConfigEditor />, { wrapper: createWrapper() });
    await userEvent.click(await screen.findByRole("button", { name: "Custom" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "Add Provider" }));
    fireEvent.change(screen.getByLabelText("Display name"), { target: { value: "HTTP gateway" } });
    fireEvent.change(screen.getByLabelText("Base URL"), { target: { value: "http://inference.test/v1" } });
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Save" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));
    await waitFor(() => expect(updateLlmConfig).toHaveBeenCalledOnce());
    const providers = updateLlmConfig.mock.calls[0][0].custom_providers;
    const id = Object.keys(providers).find(key => key !== "llm-gateway")!;
    expect(id).toMatch(/^custom-[0-9a-f-]{36}$/);
    expect(providers[id]).toMatchObject({ name: "HTTP gateway", base_url: "http://inference.test/v1" });
    expect(providers["llm-gateway"]).toEqual(response.config.custom_providers["llm-gateway"]);
    expect(updateLlmConfig.mock.calls[0][0].deepseek.http_timeouts).toEqual(response.config.deepseek.http_timeouts);
  });

  it("shows config read errors instead of staying in loading state", async () => {
    getLlmConfig.mockRejectedValue(new Error("failed to read config"));

    render(<SettingsLlmConfigEditor />, { wrapper: createWrapper() });

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
      type: "llm",
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

    render(<SettingsLlmConfigEditor />, { wrapper: createWrapper() });

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

  it("restores focus to the custom provider edit action after Escape in Strict Mode", async () => {
    const connection = { api_key: "", base_url: "http://localhost:8000/v1", protocol: "chat", models: [] };
    const providerCapabilities = { default_protocol: "chat", protocols: ["chat"] };
    getLlmConfig.mockResolvedValue({
      config: {
        ...Object.fromEntries(["anthropic", "openai", "bailian", "deepseek", "ollama", "openrouter"].map((id) => [id, connection])),
        provider: "deepseek", model: "model",
        custom_providers: { "llm-gateway": { ...connection, name: "LLM Gateway" } },
      },
      provider_keys: {},
      provider_capabilities: { anthropic: providerCapabilities, openai: providerCapabilities },
    });
    render(<StrictMode><SettingsLlmConfigEditor /></StrictMode>, { wrapper: createWrapper() });
    await userEvent.click(await screen.findByRole("button", { name: "Custom" }));
    await userEvent.click(await screen.findByRole("menuitem", { name: "LLM Gateway" }));
    const trigger = screen.getByRole("button", { name: "Edit Provider" });
    fireEvent.click(trigger);
    expect(screen.getByLabelText("Display name")).toHaveFocus();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("cancels provider deletion and stages confirmation until the page Save action", async () => {
    const response = customProviderFixture();
    getLlmConfig.mockResolvedValue(response);
    updateLlmConfig.mockImplementation(async (config) => ({ ...response, config }));
    const nativeConfirm = vi.spyOn(window, "confirm");
    await showCustomProvider();
    const trigger = screen.getByRole("button", { name: "Delete Provider" });
    fireEvent.click(trigger);
    let dialog = screen.getByRole("alertdialog", { name: "Delete Provider “LLM Gateway”?" });
    expect(dialog).toHaveAccessibleDescription(/provider and all its models.*Save changes to apply/);
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus();
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(screen.getByRole("button", { name: "Save Changes" })).toBeDisabled();
    fireEvent.click(trigger);
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(trigger).toHaveFocus());
    fireEvent.click(trigger);
    dialog = screen.getByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Custom" })).toHaveFocus());
    expect(screen.queryByRole("option", { name: "LLM Gateway" })).not.toBeInTheDocument();
    expect(updateLlmConfig).not.toHaveBeenCalled();
    expect(response.config.custom_providers["llm-gateway"].models).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));
    await waitFor(() => expect(updateLlmConfig).toHaveBeenCalledOnce());
    expect(updateLlmConfig.mock.calls[0][0].custom_providers).toEqual({});
    await waitFor(() => expect(screen.getByRole("button", { name: "Save Changes" })).toBeDisabled());
    expect(nativeConfirm).not.toHaveBeenCalled();
  });

  it("uses the same dialog for model deletion and focuses the retained Add model action", async () => {
    const response = customProviderFixture();
    getLlmConfig.mockResolvedValue(response);
    updateLlmConfig.mockImplementation(async (config) => ({ ...response, config }));
    await showCustomProvider();
    const trigger = screen.getByRole("button", { name: "Delete model" });
    fireEvent.click(trigger);
    const dialog = screen.getByRole("alertdialog", { name: "Delete model “Qwen3 32B”?" });
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(dialog).not.toBeInTheDocument();
    fireEvent.click(trigger);
    fireEvent.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Delete" }));
    expect(screen.getByText("No models configured")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Add model" })).toHaveFocus());
    expect(updateLlmConfig).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));
    await waitFor(() => expect(updateLlmConfig).toHaveBeenCalledOnce());
    expect(updateLlmConfig.mock.calls[0][0].custom_providers["llm-gateway"].models).toEqual([]);
    expect(updateLlmConfig.mock.calls[0][0].deepseek.models).toHaveLength(1);
  });

  it("edits Chat history before switching protocol and persists both only through page Save", async () => {
    const response = customProviderFixture();
    response.config.custom_providers["llm-gateway"].models[0].reasoning_replay = "reasoning_content";
    getLlmConfig.mockResolvedValue(response);
    updateLlmConfig.mockImplementation(async (config) => ({ ...response, config }));
    await showCustomProvider();
    const protocol = screen.getByRole("combobox", { name: "Protocol" });
    fireEvent.change(protocol, { target: { value: "responses" } });
    expect(protocol).toHaveValue("chat");
    expect(screen.getByRole("alert")).toHaveTextContent(/Qwen3 32B.*Protocol default/);
    expect(screen.getByRole("button", { name: "Save Changes" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Edit Model" }));
    fireEvent.click(screen.getByText("Advanced"));
    const history = screen.getByRole("combobox", { name: "Reasoning history" });
    expect(history).toHaveTextContent("Replay as reasoning_content");
    await selectOption(history, "Protocol default");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    fireEvent.change(protocol, { target: { value: "responses" } });
    expect(protocol).toHaveValue("responses");
    fireEvent.click(screen.getByRole("button", { name: "Edit Model" }));
    fireEvent.click(screen.getByText("Advanced"));
    expect(screen.queryByRole("combobox", { name: "Reasoning history" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(updateLlmConfig).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Save Changes" }));
    await waitFor(() => expect(updateLlmConfig).toHaveBeenCalledOnce());
    expect(updateLlmConfig.mock.calls[0][0].custom_providers["llm-gateway"]).toEqual(expect.objectContaining({ protocol: "responses", models: [expect.objectContaining({ reasoning_replay: "protocol_default" })] }));
    expect(updateLlmConfig.mock.calls[0][0].custom_providers["llm-gateway"]).not.toHaveProperty("adapter");
  });

  it("keeps deletion disabled for the default custom provider", async () => {
    const response = customProviderFixture();
    response.config.provider = "llm-gateway";
    getLlmConfig.mockResolvedValue(response);
    await showCustomProvider();
    const trigger = screen.getByRole("button", { name: "Delete Provider" });
    expect(trigger).toBeDisabled();
    fireEvent.click(trigger);
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(updateLlmConfig).not.toHaveBeenCalled();
  });
});
