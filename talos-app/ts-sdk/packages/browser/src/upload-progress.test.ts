import { describe, expect, it, vi } from "vitest";
import { createBrowserTalosClient, type FileUploadProgress } from "./index";

const baseUrl = "talos-cloud://server";
const metadata = { filename: "file.txt", scope: { type: "personal" as const }, tags: [], metadata: {} };
const success = () => Response.json({ status: "success", data: { file: {} } });

describe("selected fetch upload progress", () => {
  it("counts only consumed multipart bytes and reports the exact completed size", async () => {
    const onProgress = vi.fn<(progress: FileUploadProgress) => void>();
    let receivedBytes = 0;
    const send = vi.fn<typeof fetch>(async (input, init) => {
      expect(onProgress).not.toHaveBeenCalled();
      const request = new Request(input, init);
      const bytes = await request.arrayBuffer();
      receivedBytes = bytes.byteLength;
      const form = await new Response(bytes, { headers: request.headers }).formData();
      expect(JSON.parse(await (form.get("metadata") as Blob).text())).toEqual(metadata);
      expect(await (form.get("file") as Blob).text()).toBe("content");
      return success();
    });
    const client = createBrowserTalosClient({ baseUrl, fetch: send });

    await client.files.upload(metadata, new Blob(["content"]), { onProgress });

    const progress = onProgress.mock.calls.map(([value]) => value);
    expect(progress.length).toBeGreaterThan(1);
    expect(progress.slice(0, -1).every(value => value.loadedBytes > 0 && value.totalBytes === null)).toBe(true);
    expect(progress.map(value => value.loadedBytes)).toEqual(progress.map(value => value.loadedBytes).sort((a, b) => a - b));
    expect(progress.at(-1)).toEqual({ loadedBytes: receivedBytes, totalBytes: receivedBytes });
  });

  it("recreates the multipart stream after admitting the original actor", async () => {
    const onProgress = vi.fn<(progress: FileUploadProgress) => void>();
    let attempts = 0;
    let completedBytes = 0;
    const send = vi.fn<typeof fetch>(async (input, init) => {
      if ((input instanceof Request ? input.url : input.toString()).endsWith("/v1/user")) {
        return Response.json({ status: "success", data: { user: { user_id: "actor" } } });
      }
      const request = new Request(input, init);
      expect(request.headers.get("authorization")).toBe("Bearer token");
      const bytes = await request.arrayBuffer();
      if (attempts++ === 0) {
        expect(onProgress).toHaveBeenLastCalledWith({ loadedBytes: bytes.byteLength, totalBytes: bytes.byteLength });
        onProgress.mockClear();
        return new Response(null, { status: 401 });
      }
      completedBytes = bytes.byteLength;
      const form = await new Response(bytes, { headers: request.headers }).formData();
      expect(await (form.get("file") as Blob).text()).toBe("content");
      return success();
    });
    const client = createBrowserTalosClient({ baseUrl, fetch: send, getToken: () => "token", getUserId: () => "actor" });

    await client.files.upload(metadata, new Blob(["content"]), { onProgress });

    expect(send.mock.calls.map(([url]) => url)).toEqual([baseUrl + "/v1/files", baseUrl + "/v1/user", baseUrl + "/v1/files"]);
    expect(onProgress).toHaveBeenLastCalledWith({ loadedBytes: completedBytes, totalBytes: completedBytes });
  });

});
