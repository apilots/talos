import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";

describe("TalosClient files", () => {
  it("maps the files API to multipart and REST resource routes", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const client = new TalosClient(
      { request },
      { stream: vi.fn().mockResolvedValue(undefined) },
      "http://localhost:8080",
    );
    const uploadRequest = {
      filename: "diagram.png",
      scope: { type: "personal" as const },
      tags: ["diagram"],
      metadata: { source: "test" },
    };
    const uploadController = new AbortController();
    const onProgress = vi.fn();

    await client.files.upload(uploadRequest, new Uint8Array([1, 2, 3]), {
      signal: uploadController.signal,
      onProgress,
    });
    await client.files.list({
      cursor: "cursor-1",
      limit: 20,
      scope: { type: "project", projectId: "project-1" },
      tags: ["diagram", "review"],
    });
    await client.files.read({ fileId: "file/1" });
    await client.files.update({ fileId: "file/1", tags: [], metadata: {} });
    await client.files.delete({ fileId: "file/1" });

    const uploadOptions = request.mock.calls[0]?.[1] as {
      method: string;
      body: FormData;
      signal: AbortSignal;
      onUploadProgress: typeof onProgress;
    };
    expect(request.mock.calls[0]?.[0]).toBe("http://localhost:8080/v1/files");
    expect(uploadOptions.method).toBe("POST");
    expect(uploadOptions.body).toBeInstanceOf(FormData);
    const metadata = uploadOptions.body.get("metadata") as Blob;
    expect(JSON.parse(await metadata.text())).toEqual(uploadRequest);
    expect(uploadOptions.body.get("file")).toBeInstanceOf(Blob);
    expect(uploadOptions.signal).toBe(uploadController.signal);
    expect(uploadOptions.onUploadProgress).toBe(onProgress);
    expect(request).toHaveBeenNthCalledWith(
      2,
      "http://localhost:8080/v1/files?cursor=cursor-1&limit=20&scope=project&projectId=project-1&tag=diagram&tag=review",
    );
    expect(request).toHaveBeenNthCalledWith(3, "http://localhost:8080/v1/files/file%2F1");
    expect(request).toHaveBeenNthCalledWith(4, "http://localhost:8080/v1/files/file%2F1", {
      method: "PATCH",
      body: JSON.stringify({ tags: [], metadata: {} }),
    });
    expect(request).toHaveBeenNthCalledWith(5, "http://localhost:8080/v1/files/file%2F1", {
      method: "DELETE",
    });
  });
});
