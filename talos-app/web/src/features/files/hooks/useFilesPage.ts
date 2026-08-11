import { useCallback, useMemo, useState } from "react";
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { FileInfo } from "@talos/sdk-browser";

import { useConnectionKey } from "@/features/chat/hooks/useChatQueries";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { fileListQueryKey, uploadFile } from "../api";

export type FileView = "grid" | "list";
export type FileCategory = "all" | "document" | "code" | "image" | "data";

const FILES_PAGE_SIZE = 50;

export function getFileExtension(filename: string) {
  return filename.split(".").at(-1)?.toLowerCase() ?? "";
}

export function classifyFile(file: FileInfo): FileCategory {
  const extension = getFileExtension(file.originalFilename);
  if (["rs", "ts", "tsx", "py", "js", "jsx", "yaml", "yml"].includes(extension)) {
    return "code";
  }
  if (file.mimeType.startsWith("image/")) {
    return "image";
  }
  if (["csv", "json"].includes(extension)) {
    return "data";
  }
  return "document";
}

export function formatFileMeta(updatedAt: number, locale: string) {
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "2-digit",
  }).format(updatedAt * 1000);
}

export function formatFileSize(sizeBytes: number) {
  if (sizeBytes < 1024) {
    return `${sizeBytes} B`;
  }
  if (sizeBytes < 1024 * 1024) {
    return `${(sizeBytes / 1024).toFixed(1)} KiB`;
  }
  return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MiB`;
}

export function useFilesPage() {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const listKey = fileListQueryKey(connectionKey);
  const [query, setQuery] = useState("");
  const [view, setView] = useState<FileView>("grid");
  const [category, setCategory] = useState<FileCategory>("all");
  const [selectedFileId, setSelectedFileId] = useState<string | null>(null);

  const listQuery = useInfiniteQuery({
    queryKey: listKey,
    initialPageParam: undefined as string | undefined,
    queryFn: ({ pageParam }) =>
      getTalosClient().files.list({ cursor: pageParam, limit: FILES_PAGE_SIZE }),
    getNextPageParam: (page) => page.nextCursor ?? undefined,
    refetchOnWindowFocus: false,
  });
  const items = useMemo(
    () => listQuery.data?.pages.flatMap((page) => page.data) ?? [],
    [listQuery.data?.pages],
  );

  const detailQuery = useQuery({
    queryKey: ["talos", connectionKey, "file", selectedFileId],
    queryFn: () => getTalosClient().files.read({ fileId: selectedFileId! }),
    enabled: Boolean(selectedFileId),
    refetchOnWindowFocus: false,
  });

  const uploadMutation = useMutation({
    mutationFn: (file: File) =>
      uploadFile({
        file,
        scope: { type: "personal" },
      }),
    onSuccess: async (file) => {
      setSelectedFileId(file.fileId);
      queryClient.setQueryData(["talos", connectionKey, "file", file.fileId], { file });
      await queryClient.invalidateQueries({ queryKey: listKey });
    },
  });

  const updateMutation = useMutation({
    mutationFn: ({
      fileId,
      tags,
      metadata,
    }: {
      fileId: string;
      tags: string[];
      metadata: Record<string, string>;
    }) => getTalosClient().files.update({ fileId, tags, metadata }),
    onSuccess: async ({ file }) => {
      queryClient.setQueryData(["talos", connectionKey, "file", file.fileId], { file });
      await queryClient.invalidateQueries({ queryKey: listKey });
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (fileId: string) => getTalosClient().files.delete({ fileId }),
    onSuccess: async (_, fileId) => {
      if (selectedFileId === fileId) {
        setSelectedFileId(null);
      }
      queryClient.removeQueries({ queryKey: ["talos", connectionKey, "file", fileId] });
      await queryClient.invalidateQueries({ queryKey: listKey });
    },
  });

  const categorized = useMemo(() => {
    const counts: Record<FileCategory, number> = {
      all: items.length,
      document: 0,
      code: 0,
      image: 0,
      data: 0,
    };
    items.forEach((item) => {
      counts[classifyFile(item)] += 1;
    });
    return counts;
  }, [items]);

  const filteredItems = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    return items.filter((item) => {
      const matchQuery =
        !normalized ||
        [item.originalFilename, item.mimeType, ...item.tags].some((value) =>
          value.toLowerCase().includes(normalized),
        );
      const matchCategory = category === "all" || classifyFile(item) === category;
      return matchQuery && matchCategory;
    });
  }, [category, items, query]);

  const mutationError = uploadMutation.error ?? updateMutation.error ?? deleteMutation.error;

  const showAllFiles = useCallback(() => {
    setQuery("");
    setCategory("all");
    setSelectedFileId(null);
  }, []);

  return {
    query,
    setQuery,
    view,
    setView,
    category,
    setCategory,
    showAllFiles,
    categorized,
    items,
    filteredItems,
    selectedFile: detailQuery.data?.file ?? null,
    selectFile: setSelectedFileId,
    uploadFile: (file: File) => uploadMutation.mutateAsync(file),
    updateFile: (fileId: string, tags: string[], metadata: Record<string, string>) =>
      updateMutation.mutateAsync({ fileId, tags, metadata }),
    deleteFile: (fileId: string) => deleteMutation.mutateAsync(fileId),
    loadMore: () => listQuery.fetchNextPage(),
    hasMore: listQuery.hasNextPage,
    isLoading: listQuery.isLoading,
    isLoadingMore: listQuery.isFetchingNextPage,
    isLoadingDetail: detailQuery.isLoading,
    isUploading: uploadMutation.isPending,
    isUpdating: updateMutation.isPending,
    isDeleting: deleteMutation.isPending,
    error:
      listQuery.error
        ? userVisibleErrorMessage(listQuery.error, "Failed to load files.")
        : detailQuery.error
          ? userVisibleErrorMessage(detailQuery.error, "Failed to load file.")
          : mutationError
            ? userVisibleErrorMessage(mutationError, "File operation failed.")
            : null,
  };
}
