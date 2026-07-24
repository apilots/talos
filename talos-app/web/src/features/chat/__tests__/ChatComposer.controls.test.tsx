// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { ChatComposer } from "../components/ChatComposer";
import { createDefaultProjects, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import {
  getComposerInput,
  renderComposer,
  renderControlledComposer,
  renderWithQueryClient,
} from "./ChatComposer.test-utils";

const apiMocks = vi.hoisted(() => ({
  readFsDirectory: vi.fn(),
  suggestSkills: vi.fn(),
}));

vi.mock("../api", () => ({
  createFsDirectory: vi.fn(),
  readFsDirectory: apiMocks.readFsDirectory,
  suggestSkills: apiMocks.suggestSkills,
}));

describe("ChatComposer controls", () => {
  beforeEach(() => {
    apiMocks.readFsDirectory.mockResolvedValue({
      path: "/Users/xujiang",
      parent: null,
      entries: [
        {
          name: "data",
          path: "/Users/xujiang/data",
          kind: "directory",
          is_symlink: false,
          readable: true,
          writable: true,
        },
      ],
      readable: true,
      writable: true,
    });
    apiMocks.suggestSkills.mockResolvedValue([
      {
        name: "algorithmic-art",
        description: "Creating algorithmic art using p5.js with seeded randomness.",
        short_description: null,
        root: "/Users/test/.agents/skills/algorithmic-art",
        path: "/Users/test/.agents/skills/algorithmic-art/SKILL.md",
        scope: "external",
        source_plugin: null,
        source_install: null,
      },
      {
        name: "browser",
        description: "Control the in-app browser with Codex.",
        short_description: null,
        root: "/Users/test/.talos/skills/browser",
        path: "/Users/test/.talos/skills/browser/SKILL.md",
        scope: "plugin",
        source_plugin: "browser",
        source_install: "browser",
      },
    ]);
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: {
        getItem: vi.fn(() => null),
        setItem: vi.fn(),
      },
    });
  });

  afterEach(() => {
    cleanup();
    apiMocks.readFsDirectory.mockReset();
    apiMocks.suggestSkills.mockReset();
    vi.restoreAllMocks();
  });

  it("allows stopping an active response even when the composer is otherwise disabled", () => {
    const onCancel = vi.fn();
    renderComposer({
      disabled: true,
      isStreaming: true,
      onCancel,
    });

    const stopButton = screen.getByRole("button", { name: "Stop response" });
    expect((stopButton as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(stopButton);

    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("keeps the draft and blocks submission during context compaction", () => {
    const onCancel = vi.fn();
    const onSend = vi.fn();
    renderControlledComposer({ contextCompactionStage: "summarizing", initialDraft: "keep this draft", onCancel, onSend });

    expect(screen.getByRole("status")).toHaveTextContent("Context compacting... summarizing");
    fireEvent.keyDown(getComposerInput(), { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    expect(getComposerInput()).toHaveValue("keep this draft");

    fireEvent.click(screen.getByRole("button", { name: "Stop response" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("filters and executes slash commands from the composer", () => {
    const onCommand = vi.fn();
    renderControlledComposer({ initialDraft: "/c", onCommand });

    expect(screen.getByTestId("command-suggestions")).toBeInTheDocument();
    expect(screen.getByText("/compact")).toBeInTheDocument();
    expect(screen.getByText("/clear")).toBeInTheDocument();
    expect(screen.queryByText("/status")).not.toBeInTheDocument();

    fireEvent.keyDown(getComposerInput(), { key: "ArrowDown" });
    fireEvent.keyDown(getComposerInput(), { key: "Enter" });

    expect(onCommand).toHaveBeenCalledWith("clear");
    expect(getComposerInput()).toHaveValue("");
  });

  it("preserves a slash command when the session command is rejected", async () => {
    const onCommand = vi.fn().mockRejectedValue(new Error("session is busy"));
    renderControlledComposer({ initialDraft: "/clear", onCommand });

    fireEvent.keyDown(getComposerInput(), { key: "Enter" });

    await waitFor(() => expect(onCommand).toHaveBeenCalledWith("clear"));
    expect(getComposerInput()).toHaveValue("/clear");
  });

  it("preserves a new draft when an async command completes", async () => {
    let resolveCommand: (() => void) | undefined;
    const onCommand = vi.fn(() => new Promise<void>((resolve) => { resolveCommand = resolve; }));
    renderControlledComposer({ initialDraft: "/compact", onCommand });

    fireEvent.keyDown(getComposerInput(), { key: "Enter" });
    fireEvent.change(getComposerInput(), { target: { value: "keep this draft" } });
    await act(async () => resolveCommand?.());

    expect(getComposerInput()).toHaveValue("keep this draft");
  });

  it("dismisses slash command suggestions with Escape", () => {
    renderControlledComposer({ initialDraft: "/", onCommand: vi.fn() });

    expect(screen.getByTestId("command-suggestions")).toBeInTheDocument();
    fireEvent.keyDown(getComposerInput(), { key: "Escape" });

    expect(screen.queryByTestId("command-suggestions")).not.toBeInTheDocument();
  });

  it("shows full access as the current agent access mode with its description", () => {
    renderComposer();

    fireEvent.click(screen.getByRole("button", { name: /full access/i }));

    expect(screen.getByText("Agent access")).toBeInTheDocument();
    expect(
      screen.getByText("Unrestricted access to the internet and any file on your computer."),
    ).toBeInTheDocument();
  });

  it("shows skill suggestions above the composer and inserts the selected skill", async () => {
    renderControlledComposer({ initialDraft: "$" });

    expect(await screen.findByTestId("skill-suggestions")).toBeInTheDocument();
    expect(screen.getByText("Algorithmic Art")).toBeInTheDocument();
    expect(screen.getByText("Creating algorithmic art using p5.js with seeded randomness.")).toBeInTheDocument();
    expect(screen.getByText("External")).toBeInTheDocument();
    expect(screen.getByText("Plugin")).toBeInTheDocument();

    fireEvent.mouseDown(screen.getByText("Browser"));

    expect(getComposerInput()).toHaveValue("$browser ");
  });

  it("shows filtered skill suggestions for a body token after whitespace", async () => {
    renderControlledComposer({ initialDraft: "please use $br now" });
    const input = getComposerInput();
    input.setSelectionRange(14, 14);
    fireEvent.keyUp(input);

    expect(await screen.findByTestId("skill-suggestions")).toBeInTheDocument();
    expect(apiMocks.suggestSkills).toHaveBeenLastCalledWith("br");

    fireEvent.mouseDown(screen.getByText("Browser"));

    await waitFor(() => expect(input).toHaveValue("please use $browser now"));
  });

  it("does not show skill suggestions for a token joined to body text", async () => {
    renderControlledComposer({ initialDraft: "please$br" });

    await new Promise((resolve) => window.setTimeout(resolve, 160));
    expect(screen.queryByTestId("skill-suggestions")).not.toBeInTheDocument();
    expect(apiMocks.suggestSkills).not.toHaveBeenCalled();
  });

  it("keeps local runtime selected and shows cloud as unavailable", () => {
    const onRuntimeModeChange = vi.fn();
    renderComposer({ onRuntimeModeChange });

    fireEvent.click(screen.getByRole("button", { name: /local/i }));
    const cloudOption = screen.getByRole("button", { name: /cloudcoming soon/i });

    expect(cloudOption).toBeDisabled();
    fireEvent.click(cloudOption);
    expect(onRuntimeModeChange).not.toHaveBeenCalledWith("cloud");
  });

  it("shows only a disabled none control when reasoning is unsupported", () => {
    renderComposer({ reasoningEnabled: false, reasoningLevel: "none" });

    expect(screen.getByRole("button", { name: /none/i })).toBeDisabled();
    expect(screen.queryByText("Light")).not.toBeInTheDocument();
  });

  it("selects the first configured model and filters models when the provider changes", () => {
    const onModelChange = vi.fn();
    renderComposer({
      models: [
        { id: "deepseek:deepseek-v4-flash", name: "DeepSeek V4 Flash", provider: "deepseek" },
        { id: "deepseek:deepseek-v4-pro", name: "DeepSeek V4 Pro", provider: "deepseek" },
        { id: "anthropic:claude-opus", name: "Claude Opus", provider: "anthropic" },
        { id: "anthropic:claude-sonnet", name: "Claude Sonnet", provider: "anthropic" },
      ],
      onModelChange,
      selectedModel: "deepseek:deepseek-v4-flash",
    });

    fireEvent.click(screen.getByRole("button", { name: "deepseek" }));
    fireEvent.click(screen.getByRole("button", { name: "anthropic" }));

    expect(onModelChange).toHaveBeenCalledWith("anthropic:claude-opus");
    expect(screen.getByRole("button", { name: /claude opus/i })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /claude opus/i }));

    expect(screen.getByRole("button", { name: "Claude Sonnet" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "DeepSeek V4 Pro" })).not.toBeInTheDocument();
  });

  it("keeps the selected model when the current provider is selected again", () => {
    const onModelChange = vi.fn();
    renderComposer({
      models: [
        { id: "deepseek:deepseek-v4-flash", name: "DeepSeek V4 Flash", provider: "deepseek" },
        { id: "deepseek:deepseek-v4-pro", name: "DeepSeek V4 Pro", provider: "deepseek" },
        { id: "anthropic:claude-opus", name: "Claude Opus", provider: "anthropic" },
      ],
      onModelChange,
      selectedModel: "deepseek:deepseek-v4-pro",
    });

    fireEvent.click(screen.getByRole("button", { name: "deepseek" }));
    const providerButtons = screen.getAllByRole("button", { name: "deepseek" });
    fireEvent.click(providerButtons[providerButtons.length - 1]);

    expect(onModelChange).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: /deepseek v4 pro/i })).toBeInTheDocument();
  });

  it("closes the project picker when clicking outside the composer", () => {
    renderWithQueryClient(
      <div>
        <button type="button">Outside</button>
        <ChatComposer
          activeProjectId={UNBOUND_PROJECT_ID}
          draft=""
          isStreaming={false}
          models={[{ id: "mock:model-1", name: "Model 1", provider: "mock" }]}
          onCancel={vi.fn()}
          onDraftChange={vi.fn()}
          onModelChange={vi.fn()}
          onProjectChange={vi.fn()}
          onRuntimeModeChange={vi.fn()}
          onSend={vi.fn()}
          projects={createDefaultProjects()}
          runtimeMode="local"
          selectedModel="mock:model-1"
          showProjectSelector
        />
      </div>,
    );

    fireEvent.click(screen.getByRole("button", { name: /work in a project/i }));
    expect(screen.getByPlaceholderText("Search projects")).toBeInTheDocument();

    fireEvent.pointerDown(screen.getByRole("button", { name: "Outside" }));

    expect(screen.queryByPlaceholderText("Search projects")).not.toBeInTheDocument();
  });

  it("closes the project picker when clicking other composer controls", () => {
    renderComposer({ showProjectSelector: true });

    fireEvent.click(screen.getByRole("button", { name: /work in a project/i }));
    expect(screen.getByPlaceholderText("Search projects")).toBeInTheDocument();

    fireEvent.pointerDown(screen.getByRole("button", { name: /local/i }));

    expect(screen.queryByPlaceholderText("Search projects")).not.toBeInTheDocument();
  });

  it("keeps the project picker open while interacting with project search", () => {
    renderComposer({ showProjectSelector: true });

    fireEvent.click(screen.getByRole("button", { name: /work in a project/i }));
    const searchInput = screen.getByPlaceholderText("Search projects");

    fireEvent.pointerDown(searchInput);

    expect(searchInput).toBeInTheDocument();
  });

  it("opens project directory selection in a centered dialog", async () => {
    renderComposer({
      onCreateProject: vi.fn(),
      showProjectSelector: true,
    });

    fireEvent.click(screen.getByRole("button", { name: /work in a project/i }));
    fireEvent.click(screen.getByRole("button", { name: "Add new project" }));
    fireEvent.click(screen.getByRole("button", { name: "Use an existing folder" }));

    expect(screen.queryByPlaceholderText("Search projects")).not.toBeInTheDocument();
    expect(await screen.findByText("Use an existing folder")).toBeInTheDocument();
    expect(await screen.findByDisplayValue("/Users/xujiang")).toBeInTheDocument();
  });

});
