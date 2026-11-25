import { isLocalFileReference, parseFilePreviewReference } from "@talos/sdk-core";
import { normalizeWorkspaceWebpageUrl, type WorkspaceWindow } from "@/shared/store/workspace-window";
import { useAppStore } from "@/shared/store/app-store";
import { openExternalUrl } from "./open-external";

type RegisteredFileResource = Pick<Extract<WorkspaceWindow, { kind: "file" }>, "kind" | "fileId" | "nodeId">;
type SessionFileResource = Extract<WorkspaceWindow, { kind: "sessionFile" }>;
export type PreviewResource = SessionFileResource | RegisteredFileResource | { kind: "webpage"; url: string };
export type OpenResourceRequest = SessionFileResource | RegisteredFileResource | { kind: "webpage"; url: string; intent: "preview" | "external" };
export function fileReferenceId(value: string) { return /^talos-file:\/\/([A-Za-z0-9_-]+)$/.exec(value)?.[1]; }
export function resourceReference(resource: PreviewResource) { return resource.kind === "file" ? `talos-file://${resource.fileId}` : resource.kind === "sessionFile" ? resource.path : resource.url; }

const INLINE_CODE_LINE_SUFFIX_RE = /(?::[1-9]\d*(?::[1-9]\d*)?|#L[1-9]\d*)$/;
const INLINE_CODE_DISALLOWED_RE = /[\s*?"'<>|${}()[\],;=\\]/;
const PATH_EXTENSION_RE = /\.[a-z][a-z0-9]{0,9}$/i;
const BARE_FILE_EXTENSIONS = new Set([
  "html", "htm", "md", "markdown", "mdx", "pdf", "svg", "png", "jpg", "jpeg", "webp", "gif", "bmp",
  "csv", "tsv", "json", "yaml", "yml", "toml", "xml", "txt",
  "rs", "ts", "tsx", "jsx", "js", "mjs", "cjs", "css", "scss", "py", "go", "java", "kt", "swift",
  "cpp", "hpp", "sh", "bash", "zsh", "sql", "docx", "xlsx", "pptx", "odt", "ods", "odp",
]);

/** Recognize a standalone URL or file path inside inline code without matching code expressions or commands. */
export function inlineCodeResourceReference(raw: string, allowSessionFiles: boolean): string | null {
  const value = raw.trim();
  if (!value || /\s/.test(value)) return null;
  if (fileReferenceId(value)) return value;
  if (/^https?:\/\//i.test(value)) return normalizeWorkspaceWebpageUrl(value) ? value : null;
  if (!allowSessionFiles || INLINE_CODE_DISALLOWED_RE.test(value)) return null;
  if (/^file:\/\/\//i.test(value)) {
    try { return new URL(value).pathname.length > 1 ? value : null; }
    catch { return null; }
  }

  const suffix = INLINE_CODE_LINE_SUFFIX_RE.exec(value);
  const pathPart = suffix ? value.slice(0, suffix.index) : value;
  if (!pathPart || pathPart.startsWith("@") || pathPart.endsWith("/") || pathPart.endsWith(".") || pathPart.includes("//") || pathPart.includes(":") || pathPart.includes("#")) {
    return null;
  }
  const slashIndex = pathPart.lastIndexOf("/");
  const basename = slashIndex >= 0 ? pathPart.slice(slashIndex + 1) : pathPart;
  if (slashIndex >= 0) {
    return PATH_EXTENSION_RE.test(basename) || suffix !== null ? value : null;
  }
  const dotIndex = basename.lastIndexOf(".");
  if (dotIndex <= 0) return null;
  return BARE_FILE_EXTENSIONS.has(basename.slice(dotIndex + 1).toLowerCase()) ? value : null;
}

/** Registered files stay in the current workspace. Ordinary links open in the right embedded browser; explicit external intent uses the host browser. */
export async function openResource(request: OpenResourceRequest): Promise<void> {
  const store = useAppStore.getState();
  if (request.kind !== "webpage") {
    store.openWorkspaceWindow(request);
    return;
  }
  if (request.intent === "external") await openExternalUrl(request.url);
  else store.openWorkspaceWebpage(request.url);
}

export async function openResourceLink(value: string, nodeId?: string): Promise<void> {
  const fileId = fileReferenceId(value);
  if (fileId) { await openResource({ kind: "file", fileId, nodeId }); return; }
  if (isLocalFileReference(value)) {
    const context = useAppStore.getState().workspaceOwner?.context;
    if (context?.kind !== "session") throw new Error("A session is required for this file reference.");
    const reference = parseFilePreviewReference(value);
    await openResource({ kind: "sessionFile", sessionId: context.sessionId, path: reference.path,
      ...(reference.line ? { position: { kind: "text", resourcePath: null, line: reference.line, encoding: "utf-8" } } : {}) }); return;
  }
  await openResource({ kind: "webpage", url: value, intent: "preview" });
}
