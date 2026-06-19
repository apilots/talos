// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { ChatComposer } from "../components/ChatComposer";
import { createDefaultProjects, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import { getComposerInput, renderComposer, renderControlledComposer } from "./ChatComposer.test-utils";

const apiMocks = vi.hoisted(() => ({
  createFsDirectory: vi.fn(),
  readFsDirectory: vi.fn(),
  suggestSkills: vi.fn(),
}));

vi.mock("../api", () => ({
  createFsDirectory: apiMocks.createFsDirectory,
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
    apiMocks.createFsDirectory.mockImplementation(async (_parent: string, name: string) => ({
      path: `/Users/xujiang/${name}`,
      created: true,
    }));
    apiMocks.suggestSkills.mockResolvedValue([
      {
        name: "algorithmic-art",
        description: "Creating algorithmic art using p5.js with seeded randomness.",
        short_description: null,
        root: "/Users/test/.talos/skills/algorithmic-art",
        path: "/Users/test/.talos/skills/algorithmic-art/SKILL.md",
        scope: "user",
        source_plugin: null,
      },
      {
        name: "browser",
        description: "Control the in-app browser with Codex.",
        short_description: null,
        root: "/Users/test/.talos/skills/browser",
        path: "/Users/test/.talos/skills/browser/SKILL.md",
        scope: "plugin",
        source_plugin: "browser",
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
    apiMocks.createFsDirectory.mockReset();
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
    expect(screen.getByText("Personal")).toBeInTheDocument();
    expect(screen.getByText("Plugin")).toBeInTheDocument();

    fireEvent.mouseDown(screen.getByText("Browser"));

    expect(getComposerInput()).toHaveValue("$browser ");
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

  it("closes the project picker when clicking outside the composer", () => {
    render(
      <div>
        <button type="button">Outside</button>
        <ChatComposer
          activeProjectId={UNBOUND_PROJECT_ID}
          draft=""
          isStreaming={false}
          models={[{ id: "model-1", name: "Model 1" }]}
          onCancel={vi.fn()}
          onDraftChange={vi.fn()}
          onModelChange={vi.fn()}
          onProjectChange={vi.fn()}
          onRuntimeModeChange={vi.fn()}
          onSend={vi.fn()}
          projects={createDefaultProjects()}
          runtimeMode="local"
          selectedModel="model-1"
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

  it("retries project registration without creating the directory again after partial success", async () => {
    const onCreateProject = vi
      .fn()
      .mockRejectedValueOnce(new Error("Project name already exists."))
      .mockResolvedValueOnce(undefined);
    renderComposer({
      onCreateProject,
      showProjectSelector: true,
    });

    fireEvent.click(screen.getByRole("button", { name: /work in a project/i }));
    fireEvent.click(screen.getByRole("button", { name: "Add new project" }));
    fireEvent.click(screen.getByRole("button", { name: "Start from scratch" }));
    fireEvent.change(await screen.findByPlaceholderText("Folder name"), {
      target: { value: "new-project" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create and use project" }));

    expect(await screen.findByText(/Directory was created, but project registration failed/i)).toBeInTheDocument();
    expect(apiMocks.createFsDirectory).toHaveBeenCalledTimes(1);
    expect(onCreateProject).toHaveBeenCalledWith({
      directory: "/Users/xujiang/new-project",
      name: "new-project",
    });

    fireEvent.click(screen.getByRole("button", { name: "Retry project registration" }));

    await waitFor(() => expect(onCreateProject).toHaveBeenCalledTimes(2));
    expect(apiMocks.createFsDirectory).toHaveBeenCalledTimes(1);
  });
});
