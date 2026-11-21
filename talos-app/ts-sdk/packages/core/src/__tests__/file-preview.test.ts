import { expect, it, vi } from "vitest";
import { createFilePreviewClient, resolvePreviewResourcePath } from "../file-preview";
import { TalosClient } from "../client";
import { AppServerClient } from "../app-server";
import type { FilePreviewDescriptor } from "../types";
const descriptor: FilePreviewDescriptor = { previewId: "p", entryPath: "folder/report.html", resourceBaseUrl: null, document: null,
  metadata: { name: "report.html", mimeType: "text/html", sizeBytes: 1024 * 1024 + 3 }, expiresAt: 123 };
it("assembles bounded chunks once for every transport", async () => {
  const source = new Uint8Array(descriptor.metadata.sizeBytes).fill(65);
  const read = vi.fn(async ({ offset, limit }) => ({ metadata: descriptor.metadata, offset,
    dataBase64: Buffer.from(source.subarray(offset, offset + limit)).toString("base64") }));
  const preview = createFilePreviewClient({ prepare: vi.fn(), read, close: vi.fn() }, (p, path) => `native://${p.previewId}/${path}`);
  expect(await preview.content(descriptor)).toEqual(source);
  expect(read.mock.calls.map(([part]) => part)).toEqual([
    { previewId: "p", resourcePath: descriptor.entryPath, offset: 0, limit: 0 },
    { previewId: "p", resourcePath: descriptor.entryPath, offset: 0, limit: 1024 * 1024 },
    { previewId: "p", resourcePath: descriptor.entryPath, offset: 1024 * 1024, limit: 3 },
  ]);
});
it("adapts identical DTOs to HTTP and app-server without reconstructing source semantics", async () => {
  const request = vi.fn().mockResolvedValue(descriptor);
  const http = new TalosClient({ request }, { stream: vi.fn() }, "https://talos.example");
  const rpc = new AppServerClient({ request, onEvent: vi.fn() });
  const params = { source: { kind: "sessionPath" as const, sessionId: "s", path: "output/report.html" } };
  await http.files.preview.prepare(params); await rpc.files.preview.prepare(params);
  expect(request).toHaveBeenNthCalledWith(1, "https://talos.example/v1/file-previews", { method: "POST", body: JSON.stringify(params) });
  expect(request).toHaveBeenNthCalledWith(2, "file/preview/prepare", params);
  expect(http.files.preview.resourceUrl({ ...descriptor, resourceBaseUrl: "https://p.preview.example/" }, "assets/a b.png")).toBe("https://p.preview.example/assets/a%20b.png");
});

it("reads current size for changed session files and rejects changes during assembly", async () => {
  const read = vi.fn().mockResolvedValueOnce({ metadata: { ...descriptor.metadata, sizeBytes: 6 }, offset: 0, dataBase64: "" })
    .mockResolvedValueOnce({ metadata: { ...descriptor.metadata, sizeBytes: 7 }, offset: 0, dataBase64: Buffer.from("report").toString("base64") });
  const preview = createFilePreviewClient({ prepare: vi.fn(), read, close: vi.fn() }, () => "");
  await expect(preview.content(descriptor)).rejects.toThrow("Preview content changed");
  expect(read.mock.calls[1][0].limit).toBe(6);
});

it("resolves Markdown and HTML references from their document directory with standard URL rules", () => {
  expect(resolvePreviewResourcePath("docs/report.md", "images/chart.png")).toBe("docs/images/chart.png");
  expect(resolvePreviewResourcePath("docs/report.md", "../next.md")).toBe("next.md");
  expect(resolvePreviewResourcePath("docs/report.md", "/assets/a%20b.png")).toBe("assets/a b.png");
  expect(resolvePreviewResourcePath("docs/report.md", "plots/100%.png")).toBe("docs/plots/100%.png");
  expect(resolvePreviewResourcePath("docs/report.md", "plots/100%25%20done.png")).toBe("docs/plots/100% done.png");
  expect(resolvePreviewResourcePath("docs/report.md", "plots/100% done.png")).toBe("docs/plots/100% done.png");
  expect(resolvePreviewResourcePath("docs/report.md", "https://example.com/report")).toBeUndefined();
});

it("normalizes source citations against their document directory", async () => {
  const { parseFilePreviewReference } = await import("../file-preview");
  expect(parseFilePreviewReference("src/worker.rs:27:5")).toEqual({ path: "src/worker.rs", line: 27 });
  expect(parseFilePreviewReference("file:///tmp/report%20one.rs#L12")).toEqual({ path: "/tmp/report one.rs", line: 12 });
  expect(parseFilePreviewReference("a%23b.rs:12", { fromPath: "docs/report.md" })).toEqual({ path: "docs/a#b.rs", line: 12 });
  expect(parseFilePreviewReference("/workspace/project/src/lib.rs#L27", { fromPath: "docs/report.md" })).toEqual({ path: "/workspace/project/src/lib.rs", line: 27 });
  expect(parseFilePreviewReference("../src/lib.rs#L27", { fromPath: "docs/report.md" })).toEqual({ path: "src/lib.rs", line: 27 });
  expect(parseFilePreviewReference("reports/100% done.md")).toEqual({ path: "reports/100% done.md" });
  expect(parseFilePreviewReference("file:///workspace/reports/100%25%20done.md#L12")).toEqual({ path: "/workspace/reports/100% done.md", line: 12 });
});
