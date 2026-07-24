// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { renderComposer } from "./ChatComposer.test-utils";

const apiMocks = vi.hoisted(() => ({
  createFsDirectory: vi.fn(),
  readFsDirectory: vi.fn(),
}));

vi.mock("../api", () => ({
  createFsDirectory: apiMocks.createFsDirectory,
  readFsDirectory: apiMocks.readFsDirectory,
  suggestSkills: vi.fn().mockResolvedValue([]),
}));

describe("ChatComposer project creation", () => {
  beforeEach(() => {
    apiMocks.readFsDirectory.mockResolvedValue({
      path: "/Users/xujiang",
      parent: null,
      entries: [],
      readable: true,
      writable: true,
    });
    apiMocks.createFsDirectory.mockImplementation(async (_parent: string, name: string) => ({
      path: `/Users/xujiang/${name}`,
      created: true,
    }));
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
    vi.restoreAllMocks();
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

    expect(
      await screen.findByText(/Directory was created, but project registration failed/i),
    ).toBeInTheDocument();
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
