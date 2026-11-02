// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { MemoryRouter } from "react-router-dom";
import type { FileCategory } from "@/features/files";
import { FilesPage } from "./FilesPage";
import { SidebarPrimaryNav } from "@/widgets/sidebar/SidebarPrimaryNav";
import { useAppStore } from "@/shared/store/app-store";

const filesPage = vi.hoisted(() => ({
  query: "",
  setQuery: vi.fn(),
  view: "grid" as "grid" | "list",
  setView: vi.fn(),
  category: "all" as FileCategory,
  setCategory: vi.fn(),
  showAllFiles: vi.fn(),
  categorized: { all: 1, document: 0, code: 0, image: 1, data: 0 },
  items: [
    {
      fileId: "file-1",
      scope: { type: "personal" as const },
      originalFilename: "diagram.png",
      mimeType: "image/png",
      sizeBytes: 8,
      tags: [],
      metadata: {},
      createdAt: 1_776_393_847,
      updatedAt: 1_776_393_847,
    },
  ],
  filteredItems: [
    {
      fileId: "file-1",
      scope: { type: "personal" as const },
      originalFilename: "diagram.png",
      mimeType: "image/png",
      sizeBytes: 8,
      tags: [],
      metadata: {},
      createdAt: 1_776_393_847,
      updatedAt: 1_776_393_847,
    },
  ],
  selectedFile: null,
  selectFile: vi.fn(),
  uploadFile: vi.fn(),
  updateFile: vi.fn(),
  deleteFile: vi.fn(),
  loadMore: vi.fn(),
  hasMore: false,
  isLoading: false,
  isLoadingMore: false,
  isUploading: false,
  isUpdating: false,
  isDeleting: false,
  error: null,
}));

const formatFileMeta = vi.hoisted(() => vi.fn(() => "Jul 23"));

vi.mock("@/features/files", () => ({
  classifyFile: () => "image",
  FileDetailsPanel: () => null,
  formatFileMeta,
  formatFileSize: () => "8 B",
  useFilesPage: () => filesPage,
}));

describe("FilesPage", () => {
  it.each(["grid", "list"] as const)("opens a Files image in the right workspace from %s", view => {
    filesPage.view = view;
    useAppStore.setState({ workspaceOwner: null, workspaceSnapshots: {}, workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false });
    render(<MemoryRouter><FilesPage /></MemoryRouter>);
    fireEvent.click(screen.getByText("diagram.png"));
    expect(useAppStore.getState().workspaceWindows).toEqual([{ kind: "file", fileId: "file-1" }]);
    expect(useAppStore.getState().workspaceOpen).toBe(true);
  });
  beforeEach(() => {
    vi.clearAllMocks();
    filesPage.query = "";
    filesPage.view = "grid";
    filesPage.category = "all";
    filesPage.hasMore = false;
  });

  afterEach(cleanup);

  it("renders upload as the first action in the file content grid", () => {
    render(
      <MemoryRouter>
        <FilesPage />
      </MemoryRouter>,
    );

    const search = screen.getByPlaceholderText("Search files...");
    const upload = screen.getByRole("button", { name: "Upload" });

    expect(search.parentElement?.contains(upload)).toBe(false);
    expect(upload.closest(".grid")).not.toBeNull();
    expect(upload.compareDocumentPosition(screen.getByText("diagram.png"))).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(formatFileMeta).toHaveBeenCalledWith(1_776_393_847, "en");
  });

  it("renders upload as the first row in list view", () => {
    filesPage.view = "list";
    render(
      <MemoryRouter>
        <FilesPage />
      </MemoryRouter>,
    );

    const upload = screen.getByRole("button", { name: "Upload" });

    expect(upload.previousElementSibling?.textContent).toContain("Name");
    expect(upload.nextElementSibling?.textContent).toContain("diagram.png");
  });

  it("returns to the files home view when All is clicked", () => {
    render(
      <MemoryRouter>
        <FilesPage />
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole("button", { name: "All (1)" }));

    expect(filesPage.showAllFiles).toHaveBeenCalledOnce();
    expect(filesPage.setCategory).not.toHaveBeenCalled();
  });

  it("marks counts and search as partial while another page exists", () => {
    filesPage.hasMore = true;
    render(
      <MemoryRouter>
        <FilesPage />
      </MemoryRouter>,
    );

    expect(screen.getByRole("button", { name: "All (≥1)" })).not.toBeNull();
    expect(
      screen.getByText("Search and counts currently cover 1 loaded files."),
    ).not.toBeNull();
  });

  it("returns to the files home view when the active sidebar link is clicked", () => {
    filesPage.query = "diagram";
    filesPage.category = "image";
    render(
      <MemoryRouter initialEntries={["/files"]}>
        <SidebarPrimaryNav onCreateThread={vi.fn()} />
        <FilesPage />
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole("link", { name: "Files" }));

    expect(filesPage.showAllFiles).toHaveBeenCalledOnce();
  });
});
