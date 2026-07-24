// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FileInfo } from "@talos/sdk-browser";

import { classifyFile, formatFileMeta, getFileExtension, useFilesPage } from "./useFilesPage";

const fileApi = vi.hoisted(() => ({
  upload: vi.fn(),
  list: vi.fn(),
  read: vi.fn(),
  update: vi.fn(),
  delete: vi.fn(),
}));

vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({ files: fileApi }),
}));

vi.mock("@/features/chat/hooks/useChatQueries", () => ({
  useConnectionKey: () => "test-connection",
}));

const FILE: FileInfo = {
  fileId: "file-1",
  scope: { type: "personal" },
  originalFilename: "diagram.png",
  mimeType: "image/png",
  sizeBytes: 8,
  tags: ["diagram"],
  metadata: {},
  createdAt: 1_776_393_847,
  updatedAt: 1_776_393_847,
};

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return function Wrapper({ children }: PropsWithChildren) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("useFilesPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    fileApi.list.mockResolvedValue({ data: [FILE], nextCursor: null });
    fileApi.read.mockResolvedValue({ file: FILE });
    fileApi.upload.mockResolvedValue({ file: FILE });
    fileApi.update.mockResolvedValue({ file: { ...FILE, tags: ["updated"] } });
    fileApi.delete.mockResolvedValue({ fileId: FILE.fileId });
  });

  it("loads server files and reads the selected file", async () => {
    const { result } = renderHook(() => useFilesPage(), { wrapper: createWrapper() });

    await waitFor(() => expect(result.current.items).toEqual([FILE]));
    act(() => result.current.selectFile(FILE.fileId));
    await waitFor(() => expect(result.current.selectedFile).toEqual(FILE));

    expect(fileApi.read).toHaveBeenCalledWith({ fileId: FILE.fileId });
  });

  it("loads additional pages with the returned cursor", async () => {
    const secondFile = {
      ...FILE,
      fileId: "file-2",
      originalFilename: "notes.docx",
      mimeType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    };
    fileApi.list
      .mockResolvedValueOnce({ data: [FILE], nextCursor: "next-page" })
      .mockResolvedValueOnce({ data: [secondFile], nextCursor: null });
    const { result } = renderHook(() => useFilesPage(), { wrapper: createWrapper() });

    await waitFor(() => expect(result.current.hasMore).toBe(true));
    await act(async () => result.current.loadMore());

    expect(fileApi.list).toHaveBeenNthCalledWith(2, {
      cursor: "next-page",
      limit: 50,
    });
    await waitFor(() => expect(result.current.items).toEqual([FILE, secondFile]));
    expect(result.current.hasMore).toBe(false);
  });

  it("returns to the unfiltered file list from a selected file", async () => {
    const { result } = renderHook(() => useFilesPage(), { wrapper: createWrapper() });

    await waitFor(() => expect(result.current.items).toEqual([FILE]));
    act(() => {
      result.current.setQuery("diagram");
      result.current.setCategory("image");
      result.current.selectFile(FILE.fileId);
    });
    await waitFor(() => expect(result.current.selectedFile).toEqual(FILE));

    act(() => result.current.showAllFiles());

    expect(result.current.query).toBe("");
    expect(result.current.category).toBe("all");
    expect(result.current.selectedFile).toBeNull();
  });

  it("uploads one browser file through the shared files API", async () => {
    const { result } = renderHook(() => useFilesPage(), { wrapper: createWrapper() });
    const data = new File(["content"], "notes.txt", { type: "text/plain" });

    await act(async () => result.current.uploadFile(data));

    expect(fileApi.upload).toHaveBeenCalledWith(
      {
        filename: "notes.txt",
        scope: { type: "personal" },
        tags: [],
        metadata: {},
      },
      data,
      {
        signal: undefined,
        onProgress: undefined,
      },
    );
  });

  it("updates tags and metadata and deletes through deterministic file requests", async () => {
    const { result } = renderHook(() => useFilesPage(), { wrapper: createWrapper() });

    await act(async () =>
      result.current.updateFile(FILE.fileId, ["updated"], { source: "reviewed" }),
    );
    await act(async () => result.current.deleteFile(FILE.fileId));

    expect(fileApi.update).toHaveBeenCalledWith({
      fileId: FILE.fileId,
      tags: ["updated"],
      metadata: { source: "reviewed" },
    });
    expect(fileApi.delete).toHaveBeenCalledWith({ fileId: FILE.fileId });
  });
});

describe("file view helpers", () => {
  it("classifies files from their stable metadata", () => {
    expect(getFileExtension("src/App.TSX")).toBe("tsx");
    expect(classifyFile(FILE)).toBe("image");
    expect(
      classifyFile({
        ...FILE,
        originalFilename: "report.csv",
        mimeType: "text/csv",
      }),
    ).toBe("data");
  });

  it("formats database timestamps with the selected interface locale", () => {
    expect(formatFileMeta(1_784_773_122, "en")).toBe("Jul 23");
    expect(formatFileMeta(1_784_773_122, "zh-CN")).toBe("7月23日");
  });
});
