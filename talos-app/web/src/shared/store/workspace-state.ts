import { normalizeWorkspaceWebpageUrl, restoreFilePreviewPosition, workspaceWindowKey, type WorkspaceWindow } from "./workspace-window";

export type WorkspaceContext = { kind: "session"; sessionId: string } | { kind: "files" } | { kind: "general" };
export interface WorkspaceOwner { scope: string; context: WorkspaceContext }
export interface WorkspaceSnapshot {
  workspaceWindows: WorkspaceWindow[];
  workspaceActiveKey: string | null;
  workspaceOpen: boolean;
}
export const emptyWorkspace: WorkspaceSnapshot = { workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false };
export const workspaceOwnerKey = ({ scope, context }: WorkspaceOwner) => JSON.stringify([scope, context.kind, context.kind === "session" ? context.sessionId : null]);
export function workspaceBelongsToScope(key: string, scope: string): boolean {
  try { return JSON.parse(key)[0] === scope; } catch { return false; }
}
export function workspaceContextForPath(path: string): WorkspaceContext {
  const session = /^\/sessions\/([^/]+)\/?$/.exec(path);
  if (session) {
    try { return { kind: "session", sessionId: decodeURIComponent(session[1]) }; } catch { return { kind: "general" }; }
  }
  return path === "/files" ? { kind: "files" } : { kind: "general" };
}
export function snapshotWorkspace(state: WorkspaceSnapshot): WorkspaceSnapshot {
  return { workspaceWindows: state.workspaceWindows, workspaceActiveKey: state.workspaceActiveKey, workspaceOpen: state.workspaceOpen };
}

/** Restore descriptors only. Runtime objects, credentials and modal state never enter storage. */
export function restoreWorkspaces(value: unknown): Record<string, WorkspaceSnapshot> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const restored: Record<string, WorkspaceSnapshot> = {};
  for (const [owner, data] of Object.entries(value)) {
    let identity: unknown;
    try { identity = JSON.parse(owner); } catch { continue; }
    if (!Array.isArray(identity) || identity.length !== 3 || typeof identity[0] !== "string"
      || !["session", "files", "general"].includes(identity[1])
      || (identity[1] === "session" ? typeof identity[2] !== "string" : identity[2] !== null)) continue;
    if (!data || typeof data !== "object" || !Array.isArray(data.workspaceWindows)) continue;
    const windows: WorkspaceWindow[] = [];
    for (const item of data.workspaceWindows) {
      if (!item || typeof item !== "object") continue;
      let window: WorkspaceWindow | null = null;
      if (item.kind === "computer" && typeof item.nodeId === "string" && item.nodeId) window = { kind: "computer", nodeId: item.nodeId };
      if (item.kind === "file" && typeof item.fileId === "string" && item.fileId) window = { kind: "file", fileId: item.fileId };
      if (item.kind === "sessionFile" && typeof item.sessionId === "string" && item.sessionId && typeof item.path === "string" && item.path) window = { kind: "sessionFile", sessionId: item.sessionId, path: item.path };
      if (item.kind === "webpage" && typeof item.id === "string" && item.id && typeof item.url === "string"
        && (!item.url || normalizeWorkspaceWebpageUrl(item.url))) window = { kind: "webpage", id: item.id, url: item.url, title: typeof item.title === "string" ? item.title : undefined };
      if (window && (window.kind === "file" || window.kind === "sessionFile")) window.position = restoreFilePreviewPosition(item.position);
      if (window && !windows.some(entry => workspaceWindowKey(entry) === workspaceWindowKey(window))) windows.push(window);
    }
    const active = windows.some(window => workspaceWindowKey(window) === data.workspaceActiveKey)
      ? data.workspaceActiveKey as string : windows[0] ? workspaceWindowKey(windows[0]) : null;
    restored[owner] = { workspaceWindows: windows, workspaceActiveKey: active, workspaceOpen: data.workspaceOpen === true && windows.length > 0 };
  }
  return restored;
}
