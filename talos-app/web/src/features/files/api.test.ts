// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FileInfo } from "@talos/sdk-browser";

import { fileListQueryKey, uploadFile } from "./api";

const fileApi = vi.hoisted(() => ({
  upload: vi.fn(),
}));

vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({ files: fileApi }),
}));

const FILE: FileInfo = {
  fileId: "file-1",
  scope: { type: "personal" },
  originalFilename: "notes.docx",
  mimeType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  sizeBytes: 8,
  tags: [],
  metadata: {},
  createdAt: 1,
  updatedAt: 1,
};

describe("files API", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    fileApi.upload.mockResolvedValue({ file: FILE });
  });

  it("uploads one browser file through the deterministic file request", async () => {
    const file = new File(["document"], "notes.docx", { type: FILE.mimeType });
    const controller = new AbortController();
    const onProgress = vi.fn();

    await expect(
      uploadFile({
        file,
        scope: { type: "personal" },
        signal: controller.signal,
        onProgress,
      }),
    ).resolves.toEqual(FILE);

    expect(fileApi.upload).toHaveBeenCalledWith(
      {
        filename: file.name,
        scope: { type: "personal" },
        tags: [],
        metadata: {},
      },
      file,
      {
        signal: controller.signal,
        onProgress,
      },
    );
  });

  it("builds the one files-list cache key shared by Files and Composer", () => {
    expect(fileListQueryKey("connection-1")).toEqual(["talos", "connection-1", "files"]);
  });
});
