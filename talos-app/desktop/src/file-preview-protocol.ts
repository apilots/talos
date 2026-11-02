import { mkdtemp, writeFile, rm } from "node:fs/promises";
import path from "node:path";
import type { PreviewOpenResource, PreviewOpenTarget, PreviewOpenTargetId } from "@talos/sdk-app-server/desktop";
import { previewApplications, openPreviewApplication } from "./preview-applications";
import { app, protocol, type WebContents } from "electron";
import rangeParser from "range-parser";
import { createFilePreviewClient, AppProtocol, TalosApiError } from "@talos/sdk-core";
import type { DesktopAppServerController } from "./app-server-controller";

/** Register before app readiness so Chromium resolves relative URLs natively. */
export function registerFilePreviewScheme(): void {
  protocol.registerSchemesAsPrivileged([{ scheme: "talos-preview", privileges: {
    standard: true, secure: true, supportFetchAPI: true, corsEnabled: true, stream: true,
  } }]);
}

type NativePreview = {
  descriptor: AppProtocol.FilePreviewDescriptor;
  actorCredentials?: AppProtocol.ActorCredentials | null;
  ownerId: number;
  temporaryDirectories?: Set<string>;
  expiry?: NodeJS.Timeout;
};

/** Native delivery and temporary-copy lifecycle; source bytes and authority
 * remain in the shared preview service. */
export class FilePreviewProtocol {
  private readonly previews = new Map<string, NativePreview>();
  private account: string | null = null;
  private readonly observedOwners = new WeakSet<WebContents>();
  epoch = 0;
  constructor(private readonly runtime: DesktopAppServerController, private readonly onClear: () => void = () => {}) {}

  install(): void { protocol.handle("talos-preview", request => this.respond(request)); }
  private readonly installedSessions = new WeakSet<Electron.Session>();
  installSession(session: Electron.Session): void {
    if (this.installedSessions.has(session)) return;
    session.protocol.handle("talos-preview", request => this.respond(request)); this.installedSessions.add(session);
  }
  remember(descriptor: AppProtocol.FilePreviewDescriptor, actorCredentials: AppProtocol.ActorCredentials | null | undefined, owner: WebContents, epoch: number): void {
    if (epoch !== this.epoch || owner.isDestroyed()) {
      void this.release(descriptor.previewId, { descriptor, actorCredentials, ownerId: owner.id }); return;
    }
    const preview: NativePreview = { descriptor, actorCredentials, ownerId: owner.id };
    preview.expiry = setTimeout(() => this.forget(descriptor.previewId), Math.max(0, descriptor.expiresAt * 1000 - Date.now()));
    preview.expiry.unref();
    this.previews.set(descriptor.previewId, preview);
    if (!this.observedOwners.has(owner)) {
      this.observedOwners.add(owner);
      owner.once("destroyed", () => {
        for (const [id, preview] of this.previews) if (preview.ownerId === owner.id) {
          this.previews.delete(id); void this.release(id, preview);
        }
      });
    }
  }
  forget(previewId: string): void {
    const preview = this.previews.get(previewId); this.previews.delete(previewId);
    if (preview) void this.cleanup(preview);
  }
  async clear(): Promise<void> {
    this.onClear();
    this.epoch++; const previews = [...this.previews]; this.previews.clear(); this.account = null;
    await Promise.all(previews.map(([id, preview]) => this.release(id, preview)));
  }
  async accountChanged(userId: string, credentials: AppProtocol.ActorCredentials | null | undefined): Promise<void> {
    if (this.account !== null && this.account !== userId) await this.clear();
    this.account = userId;
    for (const preview of this.previews.values()) preview.actorCredentials = credentials;
  }
  private checkedResource(value: unknown, owner: WebContents): PreviewOpenResource {
    if (!value || typeof value !== "object") throw new Error("Invalid opening resource.");
    const resource = value as Record<string, unknown>;
    if (resource.kind === "webpage" && typeof resource.url === "string") {
      const url = new URL(resource.url);
      if (["http:", "https:"].includes(url.protocol) && !url.username && !url.password) return { kind: "webpage", url: url.href };
    }
    if (resource.kind === "file" && typeof resource.previewId === "string" && typeof resource.resourcePath === "string") {
      const preview = this.previews.get(resource.previewId);
      if (preview?.ownerId === owner.id && preview.descriptor.expiresAt * 1000 > Date.now()) return { kind: "file", previewId: resource.previewId, resourcePath: resource.resourcePath };
    }
    throw new TalosApiError({ code: AppProtocol.ErrorCode.ResourceNotFound, message: "The preview is unavailable. Open the file again.", localization: null });
  }
  async targets(resource: unknown, owner: WebContents): Promise<PreviewOpenTarget[]> {
    const checked = this.checkedResource(resource, owner);
    return (await previewApplications(checked.kind)).map(({ id, label, icon }) => ({ id, label, icon }));
  }
  async open(request: unknown, owner: WebContents): Promise<void> {
    if (!request || typeof request !== "object" || !("resource" in request) || !("targetId" in request) || typeof request.targetId !== "string") throw new Error("Invalid opening request.");
    const resource = this.checkedResource(request.resource, owner);
    const targets = await previewApplications(resource.kind);
    if (resource.kind === "webpage") {
      await openPreviewApplication(targets, request.targetId as PreviewOpenTargetId, resource); return;
    }
    const preview = this.previews.get(resource.previewId)!;
    const descriptor = { ...preview.descriptor, entryPath: resource.resourcePath };
    const client = createFilePreviewClient({
      prepare: async () => descriptor,
      read: part => this.runtime.invoke("file/preview/read", part, { actorCredentials: preview.actorCredentials }),
      close: part => this.runtime.invoke("file/preview/close", part, { actorCredentials: preview.actorCredentials }),
    }, () => "");
    const content = await client.stream(descriptor);
    // Account/tab changes during I/O withdraw the resource before it can launch.
    this.checkedResource(resource, owner);
    const directory = await mkdtemp(path.join(app.getPath("temp"), "talos-preview-"));
    (preview.temporaryDirectories ??= new Set()).add(directory);
    const filename = path.basename(content.metadata.name.replaceAll("\\", "/"));
    const file = path.join(directory, filename);
    try {
      await writeFile(file, content.chunks, { mode: 0o600 });
      this.checkedResource(resource, owner);
      await openPreviewApplication(targets, request.targetId as PreviewOpenTargetId, { kind: "file", path: file, directory });
    } catch (error) {
      preview.temporaryDirectories.delete(directory);
      await rm(directory, { recursive: true, force: true });
      throw error;
    }
  }
  private async cleanup(preview: NativePreview): Promise<void> {
    clearTimeout(preview.expiry);
    for (const directory of preview.temporaryDirectories ?? []) {
      try { await rm(directory, { recursive: true, force: true }); }
      catch (error) { console.error("Preview temporary file cleanup failed", error); }
    }
  }
  private async release(previewId: string, preview: NativePreview): Promise<void> {
    try { await this.runtime.invoke("file/preview/close", { previewId }, { actorCredentials: preview.actorCredentials }); }
    catch (error) {
      if (!(error instanceof TalosApiError && error.apiError.code === AppProtocol.ErrorCode.ResourceNotFound)) console.error("Preview release failed", error);
    } finally { await this.cleanup(preview); }
  }
  private async read(preview: NativePreview, previewId: string, resourcePath: string, offset: number, limit: number): Promise<AppProtocol.FilePreviewContent> {
    return this.runtime.invoke("file/preview/read", { previewId, resourcePath, offset, limit }, { actorCredentials: preview.actorCredentials });
  }
  private async respond(request: Request): Promise<Response> {
    try {
      const url = new URL(request.url);
      const id = url.hostname;
      const preview = this.previews.get(id);
      if (!preview || preview.descriptor.expiresAt * 1000 <= Date.now()) {
        this.forget(id);
        return new Response("The preview expired. Open the file again.", { status: 404 });
      }
      if (request.method !== "GET" && request.method !== "HEAD") return new Response(null, { status: 405 });
      const path = decodeURIComponent(url.pathname.replace(/^\//, ""));
      const metadata = (await this.read(preview, id, path, 0, 0)).metadata;
      const ranges = metadata.sizeBytes > 0 && request.headers.has("range") ? rangeParser(metadata.sizeBytes, request.headers.get("range")!) : undefined;
      const range = Array.isArray(ranges) && ranges.length === 1 ? ranges[0] : undefined;
      let offset = range?.start ?? 0;
      let remaining = (range?.end !== undefined ? range.end + 1 : metadata.sizeBytes) - offset;
      const headers: Record<string, string> = {
        "Content-Type": metadata.mimeType, "Content-Length": String(remaining), "Accept-Ranges": "bytes",
        "Cache-Control": "no-store", "Referrer-Policy": "no-referrer", "X-Content-Type-Options": "nosniff",
        "Content-Security-Policy": "sandbox allow-scripts allow-forms", "Access-Control-Allow-Origin": "*",
        "Access-Control-Expose-Headers": "Content-Length, Content-Range, Accept-Ranges",
      };
      if (range) headers["Content-Range"] = `bytes ${range.start}-${range.end}/${metadata.sizeBytes}`;
      const read = this.read.bind(this);
      const body = request.method === "HEAD" ? null : new ReadableStream<Uint8Array>({
        async pull(controller) {
          if (!remaining) { controller.close(); return; }
          try {
            const content = await read(preview, id, path, offset, Math.min(remaining, 1024 * 1024));
            const bytes = Buffer.from(content.dataBase64, "base64");
            if (content.offset !== offset || content.metadata.sizeBytes !== metadata.sizeBytes || !bytes.byteLength || bytes.byteLength > remaining) throw new Error("Preview content changed during the read.");
            offset += bytes.byteLength; remaining -= bytes.byteLength;
            controller.enqueue(bytes);
          } catch (error) { controller.error(error); }
        },
      });
      return new Response(body, { status: range ? 206 : 200, headers });
    } catch (error) {
      if (error instanceof TalosApiError) {
        const status = error.apiError.code === AppProtocol.ErrorCode.PermissionDenied ? 403
          : error.apiError.code === AppProtocol.ErrorCode.ResourceNotFound ? 404 : 500;
        return Response.json({ status: "error", error: error.apiError }, { status });
      }
      console.error("File preview resource request failed", error);
      return Response.json({ status: "error", error: { code: AppProtocol.ErrorCode.InternalError,
        message: "The preview resource could not be read.", localization: null } }, { status: 500 });
    }
  }
}
