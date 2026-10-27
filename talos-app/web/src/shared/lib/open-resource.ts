import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useAppStore } from "@/shared/store/app-store";
import { openExternalUrl } from "./open-external";

export type PreviewResource = { kind: "file"; fileId: string } | { kind: "webpage"; url: string };
export type OpenResourceRequest = { kind: "file"; fileId: string } | { kind: "webpage"; url: string; intent: "preview" | "link" | "external" };
export function fileReferenceId(value: string) { return /^talos-file:\/\/([A-Za-z0-9_-]+)$/.exec(value)?.[1]; }
export function resourceReference(resource: PreviewResource) { return resource.kind === "file" ? `talos-file://${resource.fileId}` : resource.url; }

/** Registered files stay in the current workspace. Link intent follows the client's real capabilities. */
export async function openResource(request: OpenResourceRequest): Promise<void> {
  const store = useAppStore.getState();
  if (request.kind === "file") {
    store.openWorkspaceWindow(request);
    return;
  }
  if (request.intent === "external" || (request.intent === "link" && getDesktopBridge())) await openExternalUrl(request.url);
  else store.openWorkspaceWebpage(request.url);
}

export async function openResourceLink(value: string): Promise<void> {
  const fileId = fileReferenceId(value);
  await openResource(fileId ? { kind: "file", fileId } : { kind: "webpage", url: value, intent: "link" });
}
