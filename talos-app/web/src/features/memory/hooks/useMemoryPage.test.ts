// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getTodayDateString, useMemoryPage } from "./useMemoryPage";

const listMemoryFiles = vi.fn();
const getMemoryFile = vi.fn();
const searchMemory = vi.fn();
const getMemoryEntry = vi.fn();
const putMemoryFile = vi.fn();

vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({
    memory: {
      list: listMemoryFiles,
      get: getMemoryFile,
      search: searchMemory,
      getEntry: getMemoryEntry,
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

    listMemoryFiles.mockResolvedValue({
      data: [
        {
          date: today,
          section_count: 1,
          updated_at: 1776393847462,
          version_token: "token-1",
        },
      ],
    });

    getMemoryFile.mockResolvedValue({
      file: {
        date: today,
        section_count: 1,
        content: "# Existing memory",
        updated_at: 1776393847462,
        version_token: "token-1",
      },
    });

    searchMemory.mockResolvedValue({
      data: [
        {
          memoryId: "mem-1",
          title: "Talos architecture",
          preview: "Talos architecture memory result",
          kind: "fact",
          importance: "normal",
          occurredAt: 1,
          rank: 1,
          source: {
            date: today,
            sessionId: null,
            fromSeq: null,
            untilSeq: null,
          },
        },
      ],
    });

    getMemoryEntry.mockResolvedValue({
      memory: {
        memoryId: "mem-1",
        title: "Talos architecture",
        body: "Complete Talos architecture memory.",
        kind: "fact",
        importance: "normal",
        occurredAt: 1,
        source: {
          date: today,
          sessionId: null,
          fromSeq: null,
          untilSeq: null,
        },
      },
    });

    putMemoryFile.mockResolvedValue({
      file: {
        date: today,
        section_count: 1,
        content: "# Updated memory",
        updated_at: 1776393847462,
        version_token: "token-2",
      },
    });
  });

  it("uses the browser-local calendar day", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 6, 21, 0, 30));

    expect(getTodayDateString()).toBe("2026-07-21");
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
        { query: "Talos", limit: 20 },
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

    expect(putMemoryFile).toHaveBeenCalledWith({
      date: getTodayDateString(),
      content: "# Updated memory",
      expected_version: "token-1",
    });
  });

  it("loads a search result by memory id before opening its daily file", async () => {
    const { result } = renderHook(() => useMemoryPage(), {
      wrapper: createWrapper(),
    });

    act(() => {
      result.current.selectMemory("mem-1");
    });

    await waitFor(() => {
      expect(getMemoryEntry).toHaveBeenCalledWith({ memoryId: "mem-1" });
      expect(result.current.selectedMemory?.body).toBe("Complete Talos architecture memory.");
    });

    act(() => {
      result.current.selectDate(getTodayDateString());
    });

    expect(result.current.selectedMemory).toBeNull();
  });
});
