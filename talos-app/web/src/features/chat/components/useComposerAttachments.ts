import { userVisibleError } from "@/shared/i18n/apiErrorMessage";
import { AppProtocol } from "@talos/sdk-browser";
import { useEffect, useMemo, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type {
  FileInfo,
  FileScope,
  FileUploadProgress,
} from "@talos/sdk-browser";

import { fileListQueryKey, uploadFile } from "@/features/files/api";
import { getTalosClient } from "@/shared/api/client";
import { useConnectionKey } from "../hooks/useChatQueries";

export type ComposerAttachmentKind = "image" | "file";

export type ComposerAttachmentItem =
  | {
      id: string;
      status: "uploading";
      file: File;
      progress: FileUploadProgress | null;
    }
  | {
      id: string;
      status: "ready";
      file: FileInfo;
    }
  | {
      id: string;
      status: "failed";
      file: File;
      error: AppProtocol.ApiError;
    };

interface UseComposerAttachmentsOptions {
  scope: FileScope;
  supportedKinds: ComposerAttachmentKind[];
  initialAttachments?: FileInfo[];
}

function attachmentKind(file: Pick<FileInfo, "mimeType">): ComposerAttachmentKind {
  return file.mimeType.startsWith("image/") ? "image" : "file";
}

function browserFileKind(file: File): ComposerAttachmentKind {
  return file.type.startsWith("image/") ? "image" : "file";
}

function fileMatchesScope(file: FileInfo, scope: FileScope) {
  if (file.scope.type === "personal") {
    return true;
  }
  return scope.type === "project" && file.scope.projectId === scope.projectId;
}

function readyItem(file: FileInfo, id = file.fileId): ComposerAttachmentItem {
  return { id, status: "ready", file };
}

export function useComposerAttachments({
  scope,
  supportedKinds,
  initialAttachments = [],
}: UseComposerAttachmentsOptions) {
  const queryClient = useQueryClient();
  const connectionKey = useConnectionKey();
  const [items, setItems] = useState<ComposerAttachmentItem[]>(() =>
    initialAttachments.map((file) => readyItem(file)),
  );
  const [availableFiles, setAvailableFiles] = useState<FileInfo[]>([]);
  const [availableNextCursor, setAvailableNextCursor] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [error, setError] = useState<AppProtocol.ApiError | null>(null);
  const controllers = useRef(new Map<string, AbortController>());
  const nextUploadId = useRef(1);
  const initialKey = initialAttachments.map((file) => file.fileId).join("\u0000");
  const previousInitialKey = useRef(initialKey);

  useEffect(() => {
    if (previousInitialKey.current === initialKey) {
      return;
    }
    previousInitialKey.current = initialKey;
    for (const controller of controllers.current.values()) {
      controller.abort();
    }
    controllers.current.clear();
    setItems(initialAttachments.map((file) => readyItem(file)));
  }, [initialAttachments, initialKey]);

  useEffect(
    () => () => {
      for (const controller of controllers.current.values()) {
        controller.abort();
      }
      controllers.current.clear();
    },
    [],
  );

  const supported = useMemo(() => new Set(supportedKinds), [supportedKinds]);
  const attachments = useMemo(
    () =>
      items.flatMap((item) =>
        item.status === "ready" ? [item.file] : [],
      ),
    [items],
  );

  function select(file: FileInfo) {
    if (!fileMatchesScope(file, scope) || !supported.has(attachmentKind(file))) {
      setError({ code: AppProtocol.ErrorCode.UnsupportedAttachment, localization: null,
        message: "The selected file is not supported by the active project and model." });
      return;
    }
    setError(null);
    setItems((current) =>
      current.some((item) => item.status === "ready" && item.file.fileId === file.fileId)
        ? current
        : [...current, readyItem(file)],
    );
  }

  async function upload(files: readonly File[]) {
    if (files.length === 0) {
      return;
    }

    setError(null);
    const newItems = files.map((file): ComposerAttachmentItem => {
      const id = `upload-${nextUploadId.current}`;
      nextUploadId.current += 1;
      if (!supported.has(browserFileKind(file))) {
        return {
          id,
          status: "failed",
          file,
          error: { code: AppProtocol.ErrorCode.UnsupportedAttachment, localization: null,
            message: "The active model does not support this attachment type." },
        };
      }
      controllers.current.set(id, new AbortController());
      return {
        id,
        status: "uploading" as const,
        file,
        progress: null,
      };
    });
    const pendingItems = newItems.filter(
      (item): item is Extract<ComposerAttachmentItem, { status: "uploading" }> =>
        item.status === "uploading",
    );
    setItems((current) => [...current, ...newItems]);

    let uploadedAny = false;
    await Promise.all(
      pendingItems.map(async (item) => {
        const controller = controllers.current.get(item.id);
        if (!controller) {
          return;
        }
        try {
          const uploaded = await uploadFile({
            file: item.file,
            scope,
            signal: controller.signal,
            onProgress: (progress) => {
              setItems((current) =>
                current.map((currentItem) =>
                  currentItem.id === item.id && currentItem.status === "uploading"
                    ? { ...currentItem, progress }
                    : currentItem,
                ),
              );
            },
          });
          if (controller.signal.aborted) {
            return;
          }
          uploadedAny = true;
          setItems((current) =>
            current.map((currentItem) =>
              currentItem.id === item.id ? readyItem(uploaded, item.id) : currentItem,
            ),
          );
        } catch (uploadError) {
          if (controller.signal.aborted) {
            return;
          }
          const error = userVisibleError(uploadError, "File upload failed.");
          setItems((current) =>
            current.map((currentItem) =>
              currentItem.id === item.id
                ? {
                    id: item.id,
                    status: "failed",
                    file: item.file,
                    error,
                  }
                : currentItem,
            ),
          );
        } finally {
          controllers.current.delete(item.id);
        }
      }),
    );
    if (uploadedAny) {
      await queryClient.invalidateQueries({
        queryKey: fileListQueryKey(connectionKey),
      });
    }
  }

  async function loadAvailable() {
    setIsLoading(true);
    setError(null);
    try {
      const response = await getTalosClient().files.list({ limit: 100 });
      setAvailableFiles(
        response.data.filter(
          (file) => fileMatchesScope(file, scope) && supported.has(attachmentKind(file)),
        ),
      );
      setAvailableNextCursor(response.nextCursor);
    } catch (listError) {
      setError(userVisibleError(listError, "Files could not be loaded."));
      throw listError;
    } finally {
      setIsLoading(false);
    }
  }

  async function loadMoreAvailable() {
    if (availableNextCursor === null) {
      return;
    }
    setIsLoadingMore(true);
    setError(null);
    try {
      const response = await getTalosClient().files.list({
        cursor: availableNextCursor,
        limit: 100,
      });
      const nextFiles = response.data.filter(
        (file) => fileMatchesScope(file, scope) && supported.has(attachmentKind(file)),
      );
      setAvailableFiles((current) => [
        ...new Map(
          [...current, ...nextFiles].map((file) => [file.fileId, file]),
        ).values(),
      ]);
      setAvailableNextCursor(response.nextCursor);
    } catch (listError) {
      setError(userVisibleError(listError, "Files could not be loaded."));
      throw listError;
    } finally {
      setIsLoadingMore(false);
    }
  }

  function remove(id: string) {
    controllers.current.get(id)?.abort();
    controllers.current.delete(id);
    setItems((current) => current.filter((item) => item.id !== id));
  }

  function clear() {
    for (const controller of controllers.current.values()) {
      controller.abort();
    }
    controllers.current.clear();
    setItems([]);
    setError(null);
  }

  const hasPendingUploads = items.some((item) => item.status === "uploading");
  const hasFailedUploads = items.some((item) => item.status === "failed");

  return {
    items,
    attachments,
    fileIds: attachments.map((file) => file.fileId),
    availableFiles,
    upload,
    loadAvailable,
    loadMoreAvailable,
    select,
    remove,
    clear,
    error,
    clearError: () => setError(null),
    isLoading,
    isLoadingMore,
    hasMoreAvailable: availableNextCursor !== null,
    hasPendingUploads,
    hasFailedUploads,
    hasInvalidAttachments: attachments.some(
      (file) => !fileMatchesScope(file, scope) || !supported.has(attachmentKind(file)),
    ),
  };
}
