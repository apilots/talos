/** Account/endpoint-scoped presentation only; never stores credentials or media grants. */
export type WorkspaceWindow =
  | ({ kind: "browser" } & import("@talos/sdk-app-server/desktop").DesktopBrowserTab)
  | { kind: "computer"; nodeId: string }
  | { kind: "markdown"; fileId: string }
  | { kind: "webpage"; id: string; url: string };

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
    case "markdown": return JSON.stringify([window.kind, window.fileId]);
    case "webpage": return JSON.stringify([window.kind, window.id]);
  }
}
