import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { getTalosClient } from "@/shared/api/client";
import { useConnectionKey } from "@/features/chat";
import type {
  ExpectedFileVersion,
  MemoryFileDetail,
} from "@talos/sdk-browser";

const MEMORY_SEARCH_TIMEOUT_MS = 30000;

function isNotFoundError(error: unknown) {
  return error instanceof Error && error.message.includes("status 404");
}

function buildMissingDetail(date: string): MemoryFileDetail {
  return {
    date,
    path: `memory/${date}.md`,
    section_count: 0,
    content: "",
    updated_at: null,
    version_token: "",
  };
}

export function getTodayDateString() {
  return new Date().toISOString().slice(0, 10);
}

export function memoryDateFromPath(path: string) {
  const fileName = path.split("/").at(-1) ?? "";
  return fileName.replace(/\.md$/, "");
}

export function formatMemoryDate(updatedAt?: number | null) {
  if (!updatedAt) {
    return "Not saved yet";
  }
  const locale = typeof navigator !== "undefined" ? navigator.language : "en";
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(updatedAt);
}

export function useMemoryPage() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [selectedDate, setSelectedDate] = useState<string>(getTodayDateString());
  const [editorContent, setEditorContent] = useState("");

  const normalizedQuery = query.trim();
  const normalizedDebouncedQuery = debouncedQuery.trim();
  const listQuery = useQuery({
    queryKey: ["talos", connectionKey, "memory-files"],
    queryFn: () => getTalosClient().memory.list({ limit: 90 }),
    refetchOnWindowFocus: false,
  });

  const searchQuery = useQuery({
    queryKey: ["talos", connectionKey, "memory-search", normalizedDebouncedQuery],
    queryFn: async ({ signal }) => {
      return await Promise.race([
        getTalosClient().memory.search(
          { q: normalizedDebouncedQuery, limit: 20 },
          { signal },
        ),
        new Promise<never>((_, reject) => {
          window.setTimeout(() => {
            reject(new Error("Memory search timed out. Try a narrower query."));
          }, MEMORY_SEARCH_TIMEOUT_MS);
        }),
      ]);
    },
    enabled: normalizedDebouncedQuery.length > 0,
    refetchOnWindowFocus: false,
    retry: false,
  });

  const detailQuery = useQuery({
    queryKey: ["talos", connectionKey, "memory-file", selectedDate],
    queryFn: async () => {
      try {
        return await getTalosClient().memory.get(selectedDate);
      } catch (error) {
        if (isNotFoundError(error)) {
          return buildMissingDetail(selectedDate);
        }
        throw error;
      }
    },
    enabled: Boolean(selectedDate),
    refetchOnWindowFocus: false,
  });

  useEffect(() => {
    if (!selectedDate && listQuery.data?.[0]?.date) {
      setSelectedDate(listQuery.data[0].date);
    }
  }, [listQuery.data, selectedDate]);

  useEffect(() => {
    if (!normalizedQuery) {
      setDebouncedQuery("");
      return;
    }

    const handle = window.setTimeout(() => {
      setDebouncedQuery(normalizedQuery);
    }, 450);

    return () => {
      window.clearTimeout(handle);
    };
  }, [normalizedQuery]);

  useEffect(() => {
    setEditorContent(detailQuery.data?.content ?? "");
  }, [detailQuery.data?.date, detailQuery.data?.version_token, detailQuery.data?.content]);

  const saveMutation = useMutation({
    mutationFn: async () => {
      const detail = detailQuery.data ?? buildMissingDetail(selectedDate);
      const baseVersion: ExpectedFileVersion = detail.version_token
        ? { mode: "match", version_token: detail.version_token }
        : { mode: "must_not_exist" };
      return getTalosClient().memory.put(selectedDate, {
        content: editorContent,
        base_version: baseVersion,
      });
    },
    onSuccess: async (detail) => {
      queryClient.setQueryData(["talos", connectionKey, "memory-file", detail.date], detail);
      await queryClient.invalidateQueries({ queryKey: ["talos", connectionKey, "memory-files"] });
      await queryClient.invalidateQueries({ queryKey: ["talos", connectionKey, "memory-search"] });
    },
  });

  const files = listQuery.data ?? [];
  const searchResults = searchQuery.data ?? [];
  const selectedDetail = detailQuery.data ?? buildMissingDetail(selectedDate);
  const isDirty = editorContent !== selectedDetail.content;
  const isSearchPending = normalizedQuery.length > 0 && normalizedDebouncedQuery !== normalizedQuery;

  const searchResultDates = useMemo(
    () =>
      Array.from(
        new Map(
          searchResults.map((result) => [memoryDateFromPath(result.path), memoryDateFromPath(result.path)]),
        ).values(),
      ),
    [searchResults],
  );

  function selectDate(date: string) {
    setSelectedDate(date);
  }

  function openToday() {
    setSelectedDate(getTodayDateString());
  }

  return {
    query,
    setQuery,
    files,
    searchResults,
    searchResultDates,
    selectedDate,
    selectedDetail,
    editorContent,
    setEditorContent,
    selectDate,
    openToday,
    saveCurrent: () => saveMutation.mutateAsync(),
    isDirty,
    isLoadingList: listQuery.isLoading,
    isLoadingDetail: detailQuery.isLoading,
    isSearching: searchQuery.isFetching,
    isSearchPending,
    isSaving: saveMutation.isPending,
    searchError: searchQuery.error instanceof Error ? searchQuery.error.message : null,
    saveError: saveMutation.error instanceof Error ? saveMutation.error.message : null,
  };
}
