// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getTodayDateString, memoryDateFromPath, useMemoryPage } from "./useMemoryPage";

const listMemoryFiles = vi.fn();
const getMemoryFile = vi.fn();
const searchMemory = vi.fn();
const putMemoryFile = vi.fn();

vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({
    memory: {
      list: listMemoryFiles,
      get: getMemoryFile,
      search: searchMemory,
      put: putMemoryFile,
    },
  }),
}));

vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "test-connection",
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
      mutations: {
        retry: false,
      },
    },
  });

  return function Wrapper({ children }: PropsWithChildren) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("useMemoryPage helpers", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useRealTimers();

    const today = getTodayDateString();

    listMemoryFiles.mockResolvedValue([
      {
        date: today,
        path: `memory/${today}.md`,
        section_count: 1,
        updated_at: 1776393847462,
        version_token: "token-1",
      },
    ]);

    getMemoryFile.mockResolvedValue({
      date: today,
      path: `memory/${today}.md`,
      section_count: 1,
      content: "# Existing memory",
      updated_at: 1776393847462,
      version_token: "token-1",
    });

    searchMemory.mockResolvedValue([
      {
        path: `memory/${today}.md`,
        snippet: "Talos architecture memory result",
        start_line: 1,
        end_line: 4,
      },
    ]);

    putMemoryFile.mockResolvedValue({
      date: today,
      path: `memory/${today}.md`,
      section_count: 1,
      content: "# Updated memory",
      updated_at: 1776393847462,
      version_token: "token-2",
    });
  });

  it("extracts memory file dates from search result paths", () => {
    expect(memoryDateFromPath("memory/2026-04-17.md")).toBe("2026-04-17");
  });

  it("returns an ISO day string for today", () => {
    expect(getTodayDateString()).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it("calls memory search and derives dates from search results", async () => {
    const { result } = renderHook(() => useMemoryPage(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.files).toHaveLength(1);
      expect(result.current.selectedDetail.version_token).toBe("token-1");
    });

    act(() => {
      result.current.setQuery("Talos");
    });

    expect(searchMemory).not.toHaveBeenCalled();

    await act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 500));
    });

    await waitFor(() => {
      expect(searchMemory).toHaveBeenCalledWith(
        { q: "Talos", limit: 20 },
        expect.objectContaining({ signal: expect.any(AbortSignal) }),
      );
    });

    await waitFor(() => {
      expect(result.current.searchResults).toHaveLength(1);
      expect(result.current.searchResultDates).toEqual([getTodayDateString()]);
    });
  });

  it("saves the current memory file with version matching", async () => {
    const { result } = renderHook(() => useMemoryPage(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.selectedDetail.version_token).toBe("token-1");
    });

    act(() => {
      result.current.setEditorContent("# Updated memory");
    });

    await act(async () => {
      await result.current.saveCurrent();
    });

    expect(putMemoryFile).toHaveBeenCalledWith(getTodayDateString(), {
      content: "# Updated memory",
      base_version: {
        mode: "match",
        version_token: "token-1",
      },
    });
  });
});
