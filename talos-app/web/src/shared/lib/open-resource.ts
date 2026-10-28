import { parseFilePreviewReference } from "@talos/sdk-core";
import type { WorkspaceWindow } from "@/shared/store/workspace-window";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useAppStore } from "@/shared/store/app-store";
import { openExternalUrl } from "./open-external";

type SessionFileResource = Extract<WorkspaceWindow, { kind: "sessionFile" }>;
export type PreviewResource = SessionFileResource | { kind: "file"; fileId: string } | { kind: "webpage"; url: string };
export type OpenResourceRequest = SessionFileResource | { kind: "file"; fileId: string } | { kind: "webpage"; url: string; intent: "preview" | "link" | "external" };
export function fileReferenceId(value: string) { return /^talos-file:\/\/([A-Za-z0-9_-]+)$/.exec(value)?.[1]; }
export function resourceReference(resource: PreviewResource) { return resource.kind === "file" ? `talos-file://${resource.fileId}` : resource.kind === "sessionFile" ? resource.path : resource.url; }

/** Registered files stay in the current workspace. Link intent follows the client's real capabilities. */
export async function openResource(request: OpenResourceRequest): Promise<void> {
  const store = useAppStore.getState();
  if (request.kind !== "webpage") {
    store.openWorkspaceWindow(request);
    return;
  }
  if (request.intent === "external" || (request.intent === "link" && getDesktopBridge())) await openExternalUrl(request.url);
  else store.openWorkspaceWebpage(request.url);
}

export async function openResourceLink(value: string): Promise<void> {
  const fileId = fileReferenceId(value);
  if (fileId) { await openResource({ kind: "file", fileId }); return; }
  if (value && (!/^[a-z][a-z\d+.-]*:/i.test(value) || value.startsWith("file://"))) {
    const context = useAppStore.getState().workspaceOwner?.context;
    if (context?.kind !== "session") throw new Error("A session is required for this file reference.");
    const reference = parseFilePreviewReference(value);
    await openResource({ kind: "sessionFile", sessionId: context.sessionId, path: reference.path,
      ...(reference.line ? { position: { kind: "text", resourcePath: null, line: reference.line, encoding: "utf-8" } } : {}) }); return;
  }
  await openResource({ kind: "webpage", url: value, intent: "link" });
}
