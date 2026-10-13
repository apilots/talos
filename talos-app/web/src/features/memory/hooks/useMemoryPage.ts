import { useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { getTalosClient } from "@/shared/api/client";
import { useEnvironmentKey } from "@/features/computers";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { AppProtocol, TalosApiError, type MemoryFileDetail } from "@talos/sdk-browser";

const MEMORY_SEARCH_TIMEOUT_MS = 30000;
const MEMORY_NOT_FOUND_CODES = [
  AppProtocol.ErrorCode.ResourceNotFound,
  AppProtocol.ErrorCode.SessionNotFound,
  AppProtocol.ErrorCode.PathNotFound,
] as const satisfies readonly AppProtocol.ErrorCode[];

function isNotFoundError(error: unknown) {
  return (
    error instanceof TalosApiError &&
    MEMORY_NOT_FOUND_CODES.some((code) => code === error.code)
  );
}

function buildMissingDetail(date: string): MemoryFileDetail {
  return {
    date,
    section_count: 0,
    content: "",
    updated_at: null,
    version_token: "",
  };
}

export function getTodayDateString() {
  const today = new Date();
  const year = today.getFullYear();
  const month = String(today.getMonth() + 1).padStart(2, "0");
  const day = String(today.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
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
  const connectionKey = useEnvironmentKey();
  const queryClient = useQueryClient();
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [selectedDate, setSelectedDate] = useState<string>(getTodayDateString());
  const [selectedMemoryId, setSelectedMemoryId] = useState<string | null>(null);
  const [editorContent, setEditorContent] = useState("");

  const normalizedQuery = query.trim();
  const normalizedDebouncedQuery = debouncedQuery.trim();
  const listQuery = useQuery({
    queryKey: ["talos", connectionKey, "memory-files"],
    queryFn: async () => {
      const response = await getTalosClient().memory.list({ limit: 90 });
      return response.data;
    },
    refetchOnWindowFocus: false,
  });

  const searchQuery = useQuery({
    queryKey: ["talos", connectionKey, "memory-search", normalizedDebouncedQuery],
    queryFn: async ({ signal }) => {
      return await Promise.race([
        getTalosClient()
          .memory.search(
            { query: normalizedDebouncedQuery, limit: 20 },
            { signal },
          )
          .then((response) => response.data),
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
        const response = await getTalosClient().memory.get({ date: selectedDate });
        return response.file;
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

  const memoryEntryQuery = useQuery({
    queryKey: ["talos", connectionKey, "memory-entry", selectedMemoryId],
    queryFn: async () => {
      const response = await getTalosClient().memory.getEntry({ memoryId: selectedMemoryId! });
      return response.memory;
    },
    enabled: Boolean(selectedMemoryId),
    refetchOnWindowFocus: false,
    retry: false,
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
    setSelectedMemoryId(null);
  }, [normalizedQuery]);

  useEffect(() => {
    setEditorContent(detailQuery.data?.content ?? "");
  }, [detailQuery.data?.date, detailQuery.data?.version_token, detailQuery.data?.content]);

  const saveMutation = useMutation({
    mutationFn: async () => {
      const detail = detailQuery.data ?? buildMissingDetail(selectedDate);
      const response = await getTalosClient().memory.put({
        date: selectedDate,
        content: editorContent,
        ...(detail.version_token ? { expected_version: detail.version_token } : {}),
      });
      return response.file;
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
          searchResults.map((result) => [result.source.date, result.source.date]),
        ).values(),
      ),
    [searchResults],
  );

  function selectDate(date: string) {
    setSelectedDate(date);
    setSelectedMemoryId(null);
    setQuery("");
    setDebouncedQuery("");
  }

  function selectMemory(memoryId: string) {
    setSelectedMemoryId(memoryId);
  }

  function openToday() {
    selectDate(getTodayDateString());
  }

  return {
    query,
    setQuery,
    files,
    searchResults,
    searchResultDates,
    selectedMemory: memoryEntryQuery.data ?? null,
    selectedDate,
    selectedDetail,
    editorContent,
    setEditorContent,
    selectDate,
    selectMemory,
    openToday,
    saveCurrent: () => saveMutation.mutateAsync(),
    isDirty,
    isLoadingList: listQuery.isLoading,
    isLoadingDetail: detailQuery.isLoading,
    isLoadingMemory: memoryEntryQuery.isLoading,
    isSearching: searchQuery.isFetching,
    isSearchPending,
    isSaving: saveMutation.isPending,
    searchError: searchQuery.error
      ? userVisibleErrorMessage(searchQuery.error, "Memory search failed.")
      : null,
    memoryError: memoryEntryQuery.error
      ? userVisibleErrorMessage(memoryEntryQuery.error, "Failed to load memory.")
      : null,
    saveError: saveMutation.error
      ? userVisibleErrorMessage(saveMutation.error, "Failed to save memory.")
      : null,
  };
}
