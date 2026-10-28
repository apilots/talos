import { decodeFileBytes } from "./file-content";
import type { FilePreviewPrepareRequest, FilePreviewDescriptor, FilePreviewReadRequest, FilePreviewContent, FilePreviewCloseRequest, FilePreviewCloseResponse, FilePreviewMetadata } from "./types";

export interface FilePreviewTransport {
  prepare(request: FilePreviewPrepareRequest): Promise<FilePreviewDescriptor>;
  read(request: FilePreviewReadRequest): Promise<FilePreviewContent>;
  close(request: FilePreviewCloseRequest): Promise<FilePreviewCloseResponse>;
}

/** A single-use byte stream with current source metadata. */
export interface FilePreviewStream { metadata: FilePreviewMetadata; chunks: AsyncIterable<Uint8Array> }

/** Shared resource operations; shells supply only their resource URL adapter. */
export function createFilePreviewClient(transport: FilePreviewTransport, resourceUrl: (preview: FilePreviewDescriptor, path: string) => string) {
  const readBytes = async (request: FilePreviewReadRequest) => {
    const part = await transport.read(request);
    const data = decodeFileBytes(part.dataBase64);
    if (part.offset !== request.offset || data.byteLength > request.limit || (request.limit > 0 && request.offset < part.metadata.sizeBytes && data.byteLength === 0)) throw new Error("Preview content changed during the read.");
    return { metadata: part.metadata, offset: part.offset, data };
  };
  const stream = async (preview: FilePreviewDescriptor, path = preview.entryPath): Promise<FilePreviewStream> => {
    const metadata = (await readBytes({ previewId: preview.previewId, resourcePath: path, offset: 0, limit: 0 })).metadata;
    async function* chunks() {
      for (let offset = 0; offset < metadata.sizeBytes;) {
        const part = await readBytes({ previewId: preview.previewId, resourcePath: path, offset, limit: Math.min(1024 * 1024, metadata.sizeBytes - offset) });
        if (part.metadata.sizeBytes !== metadata.sizeBytes) throw new Error("Preview content changed during the read.");
        offset += part.data.byteLength;
        yield part.data;
      }
    }
    return { metadata, chunks: chunks() };
  };
  return {
    ...transport,
    readBytes,
    stream,
    resolvePath: resolvePreviewResourcePath,
    resourceUrl: (preview: FilePreviewDescriptor, path = preview.entryPath) => resourceUrl(preview, path),
    async content(preview: FilePreviewDescriptor, path = preview.entryPath): Promise<Uint8Array> {
      const resource = await stream(preview, path);
      const bytes = new Uint8Array(resource.metadata.sizeBytes);
      let offset = 0;
      for await (const part of resource.chunks) { bytes.set(part, offset); offset += part.byteLength; }
      return bytes;
    },
  };
}

/** Resolve a document reference with the browser's URL rules, within its root.
 * External schemes remain the caller's explicit link-opening policy. */
export function resolvePreviewResourcePath(fromPath: string, reference: string): string | undefined {
  if (/^[a-z][a-z\d+.-]*:/i.test(reference) || reference.startsWith("//")) return undefined;
  const base = new URL(fromPath.split("/").map(encodeURIComponent).join("/"), "https://preview.invalid/");
  return decodeURIComponent(new URL(reference, base).pathname.slice(1));
}

export interface FilePreviewReference { path: string; line?: number }
/** File citation syntax is resolved once for all shells. It never selects a
 * Worker or authorizes a root; those remain the saved session's responsibility. */
export function parseFilePreviewReference(reference: string, options: { fromPath?: string } = {}): FilePreviewReference {
  if (/^[a-z][a-z\d+.-]*:/i.test(reference) && !/^file:\/\//i.test(reference)) return { path: reference };
  let path = reference;
  let hash = "";
  if (/^file:\/\//i.test(reference)) {
    const url = new URL(reference);
    if (url.hostname && url.hostname !== "localhost") throw new Error("Remote file URL hosts are unsupported.");
    path = decodeURIComponent(url.pathname).replace(/^\/([A-Za-z]:\/)/, "$1"); hash = url.hash;
  } else {
    const index = reference.indexOf("#");
    if (index >= 0) { path = reference.slice(0, index); hash = reference.slice(index); }
  }
  const suffix = /^(.*?):([1-9]\d*)(?::\d+)?$/.exec(path);
  const line = /^#L([1-9]\d*)$/.exec(hash)?.[1] ?? suffix?.[2];
  const raw = suffix?.[1] ?? path;
  const resolved = /^file:\/\//i.test(reference) ? raw : options.fromPath !== undefined ? resolvePreviewResourcePath(options.fromPath, raw) ?? raw : decodeURIComponent(raw);
  return { path: resolved, ...(line && Number.isSafeInteger(Number(line)) ? { line: Number(line) } : {}) };
}
