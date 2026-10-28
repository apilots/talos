/** Account/endpoint-scoped presentation only; never stores credentials or media grants. */
export type FilePreviewPosition =
  | { kind: "archive"; resourcePath: string | null }
  | { kind: "text"; resourcePath: string | null; line: number; encoding: "utf-8" | "gb18030" | "utf-16le" }
  | { kind: "pdf"; resourcePath: string; page: number; scale: number }
  | { kind: "markdown"; resourcePath: string; scrollTop: number };
export type WorkspaceWindow =
  | ({ kind: "browser" } & import("@talos/sdk-app-server/desktop").DesktopBrowserTab)
  | { kind: "computer"; nodeId: string }
  | { kind: "file"; fileId: string; position?: FilePreviewPosition }
  | { kind: "sessionFile"; sessionId: string; path: string; position?: FilePreviewPosition }
  | { kind: "webpage"; id: string; url: string; title?: string };

export function normalizeWorkspaceWebpageUrl(value: string): string | null {
  try {
    const url = new URL(value);
    return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password ? url.href : null;
  } catch { return null; }
}

export function workspaceWindowKey(window: WorkspaceWindow): string {
  switch (window.kind) {
    case "browser": return JSON.stringify([window.kind, window.id]);
    case "computer": return JSON.stringify([window.kind, window.nodeId]);
    case "file": return JSON.stringify([window.kind, window.fileId]);
    case "sessionFile": return JSON.stringify([window.kind, window.sessionId, window.path]);
    case "webpage": return JSON.stringify([window.kind, window.id]);
  }
}

export function restoreFilePreviewPosition(value: unknown): FilePreviewPosition | undefined {
  if (!value || typeof value !== "object" || !("kind" in value) || !("resourcePath" in value)) return;
  if (value.kind === "archive" && (value.resourcePath === null || typeof value.resourcePath === "string")) return { kind: "archive", resourcePath: value.resourcePath };
  if (value.resourcePath !== null && typeof value.resourcePath !== "string") return;
  const v = value as Record<string, unknown>;
  if (v.kind === "text" && typeof v.line === "number" && Number.isInteger(v.line) && v.line >= 1 && (v.encoding === "utf-8" || v.encoding === "gb18030" || v.encoding === "utf-16le")) return { kind: "text", resourcePath: value.resourcePath, line: v.line, encoding: v.encoding };
  if (typeof value.resourcePath !== "string") return;
  if (v.kind === "pdf" && typeof v.page === "number" && Number.isInteger(v.page) && v.page >= 1 && typeof v.scale === "number" && v.scale >= 0.5 && v.scale <= 3) return { kind: "pdf", resourcePath: value.resourcePath, page: v.page, scale: v.scale };
  if (v.kind === "markdown" && typeof v.scrollTop === "number" && Number.isFinite(v.scrollTop) && v.scrollTop >= 0) return { kind: "markdown", resourcePath: value.resourcePath, scrollTop: v.scrollTop };
}
