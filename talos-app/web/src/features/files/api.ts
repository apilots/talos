import type {
  FileInfo,
  FileScope,
  FileUploadProgress,
} from "@talos/sdk-browser";

import { getTalosClient } from "@/shared/api/client";

export interface UploadFileRequest {
  file: File;
  scope: FileScope;
  signal?: AbortSignal;
  onProgress?: (progress: FileUploadProgress) => void;
}

export function fileListQueryKey(connectionKey: string) {
  return ["talos", connectionKey, "files"] as const;
}

export function isMarkdownFile(file: Pick<FileInfo, "originalFilename" | "mimeType">) {
  const extension = file.originalFilename.split(".").at(-1)?.toLowerCase();
  return extension === "md" || extension === "markdown"
    || ["text/markdown", "text/x-markdown"].includes(file.mimeType.split(";")[0].trim().toLowerCase());
}

export async function uploadFile({
  file,
  scope,
  signal,
  onProgress,
}: UploadFileRequest): Promise<FileInfo> {
  const response = await getTalosClient().files.upload(
    {
      filename: file.name,
      scope,
      tags: [],
      metadata: {},
    },
    file,
    { signal, onProgress },
  );
  return response.file;
}
