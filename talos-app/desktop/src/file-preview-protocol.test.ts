import { tmpdir } from "node:os";
import { readFile, stat } from "node:fs/promises";
import { EventEmitter } from "node:events";
import type { WebContents } from "electron";
import { beforeEach, expect, it, vi } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopAppServerController } from "./app-server-controller";
const electron = vi.hoisted(() => ({ handle: vi.fn(), registerSchemesAsPrivileged: vi.fn(), open: vi.fn() }));
vi.mock("electron", () => ({ protocol: electron, app: { getPath: () => tmpdir() } }));
vi.mock("./preview-applications", () => ({ previewApplications: async () => [{ id: "system", label: "System default", kind: "system" }], openPreviewApplication: electron.open }));
import { FilePreviewProtocol, registerFilePreviewScheme } from "./file-preview-protocol";
const descriptor: AppProtocol.FilePreviewDescriptor = { previewId: "preview-id", entryPath: "report.html", resourceBaseUrl: null, document: null,
  metadata: { name: "report.html", mimeType: "text/html", sizeBytes: 6 }, expiresAt: Math.floor(Date.now() / 1000) + 3600 };
function setup(source = Buffer.from("report")) {
  const current = { ...descriptor, metadata: { ...descriptor.metadata, sizeBytes: source.byteLength } };
  const invoke = vi.fn(async (_method, request) => ({ metadata: current.metadata, offset: request.offset,
    dataBase64: source.subarray(request.offset, request.offset + request.limit).toString("base64") }));
  const protocol = new FilePreviewProtocol({ invoke } as unknown as DesktopAppServerController);
  protocol.install();
  const owner = Object.assign(new EventEmitter(), { id: 1, isDestroyed: () => false }) as unknown as WebContents;
  const credentials: AppProtocol.ActorCredentials = { credential: { type: "existing_session_token", token: "actor-token" } };
  protocol.remember(current, credentials, owner, protocol.epoch);
  const respond = electron.handle.mock.calls.at(-1)![1] as (request: Request) => Promise<Response>;
  return { protocol, owner, respond, invoke, credentials };
}
beforeEach(() => vi.clearAllMocks());
it("streams native resources through the authenticated canonical range API", async () => {
  registerFilePreviewScheme();
  expect(electron.registerSchemesAsPrivileged.mock.calls[0][0][0].privileges.standard).toBe(true);
  const { respond, invoke, credentials } = setup();
  const response = await respond(new Request("talos-preview://preview-id/report.html", { headers: { range: "bytes=1-3" } }));
  expect(response.status).toBe(206);
  expect(response.headers.get("content-range")).toBe("bytes 1-3/6");
  expect(await response.text()).toBe("epo");
  expect(invoke).toHaveBeenLastCalledWith("file/preview/read", { previewId: "preview-id", resourcePath: "report.html", offset: 1, limit: 3 }, { actorCredentials: credentials });
});
it("uses one owner listener and withdraws resources on close, owner destruction and account change", async () => {
  const { protocol, respond, owner, credentials, invoke } = setup();
  protocol.remember(descriptor, credentials, owner, protocol.epoch);
  expect(owner.listenerCount("destroyed")).toBe(1);
  protocol.forget(descriptor.previewId);
  expect((await respond(new Request("talos-preview://preview-id/report.html"))).status).toBe(404);
  protocol.remember(descriptor, credentials, owner, protocol.epoch);
  owner.emit("destroyed");
  expect((await respond(new Request("talos-preview://preview-id/report.html"))).status).toBe(404);
  expect(invoke).toHaveBeenCalledWith("file/preview/close", { previewId: descriptor.previewId }, { actorCredentials: credentials });
  await protocol.accountChanged("a", credentials);
  protocol.remember(descriptor, credentials, owner, protocol.epoch);
  await protocol.accountChanged("b", null);
  expect((await respond(new Request("talos-preview://preview-id/report.html"))).status).toBe(404);
});

it("opens a service-owned copy and removes it with its preview lifecycle", async () => {
  const source = Buffer.alloc(1024 * 1024 + 17, 65);
  const { protocol, owner, invoke } = setup(source);
  await protocol.open({ resource: { kind: "file", previewId: descriptor.previewId, resourcePath: descriptor.entryPath }, targetId: "system" }, owner);
  const copy = electron.open.mock.calls[0][2];
  expect(await readFile(copy.path)).toEqual(source);
  expect(invoke.mock.calls.filter(([method]) => method === "file/preview/read").map(([, request]) => request.limit)).toEqual([0, 1024 * 1024, 17]);
  await protocol.clear();
  await expect(stat(copy.directory)).rejects.toMatchObject({ code: "ENOENT" });
});
