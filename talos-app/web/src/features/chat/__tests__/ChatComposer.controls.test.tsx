// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { ChatComposer } from "../components/ChatComposer";
import { createDefaultProjects, UNBOUND_PROJECT_ID } from "@/shared/types/domain";
import {
  renderComposer,
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
      entries: [{
        name: "data",
        path: "/Users/xujiang/data",
        kind: "directory",
        is_symlink: false,
        readable: true,
        writable: true,
      }],
      readable: true,
      writable: true,
    });
    apiMocks.suggestSkills.mockResolvedValue([]);
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
    vi.restoreAllMocks();
  });

  it("changes the current agent access mode", () => {
    const onPermissionModeChange = vi.fn();
    renderComposer({ permissionMode: "askForApproval", onPermissionModeChange });

    fireEvent.click(screen.getByRole("button", { name: /ask for approval/i }));
    fireEvent.click(screen.getByRole("button", { name: /approve for me/i }));

    expect(onPermissionModeChange).toHaveBeenCalledWith("approveForMe");
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
          onSend={vi.fn()}
          projects={createDefaultProjects()}
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

  it("keeps the project picker open while using project search", () => {
    renderComposer({ showProjectSelector: true });

    fireEvent.click(screen.getByRole("button", { name: /work in a project/i }));
    const searchInput = screen.getByPlaceholderText("Search projects");
    fireEvent.pointerDown(searchInput);

    expect(searchInput).toBeInTheDocument();
  });

  it("opens project directory selection in a dialog", async () => {
    renderComposer({ onCreateProject: vi.fn(), showProjectSelector: true });

    fireEvent.click(screen.getByRole("button", { name: /work in a project/i }));
    fireEvent.click(screen.getByRole("button", { name: "Add new project" }));
    fireEvent.click(screen.getByRole("button", { name: "Use an existing folder" }));

    expect(await screen.findByDisplayValue("/Users/xujiang")).toBeInTheDocument();
  });
});
