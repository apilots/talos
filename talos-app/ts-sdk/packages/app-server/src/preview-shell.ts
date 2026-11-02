/** Native presentation capabilities. File authority always comes from the
 * canonical preview grant; callers never supply native file/application paths. */
export type PreviewOpenTargetId = "system" | "chrome" | "safari" | "firefox" | "edge" | "vscode" | "iterm" | "terminal";
export type PreviewOpenResource = { kind: "webpage"; url: string } | { kind: "file"; previewId: string; resourcePath: string };
export interface PreviewOpenTarget { id: PreviewOpenTargetId; label: string; icon?: string }
export interface DesktopPreviewBridge {
  browser(operation: PreviewBrowserOperation): Promise<PreviewBrowserState | null>;
  onBrowserState(handler: (state: PreviewBrowserState) => void): () => void;
  targets(resource: PreviewOpenResource): Promise<PreviewOpenTarget[]>;
  open(request: { resource: PreviewOpenResource; targetId: PreviewOpenTargetId }): Promise<void>;
}

export interface PreviewBrowserBounds { x: number; y: number; width: number; height: number }
export type PreviewBrowserOperation =
  | { kind: "mount"; id: string; url: string; bounds: PreviewBrowserBounds; visible: boolean }
  | { kind: "layout"; id: string; bounds: PreviewBrowserBounds; visible: boolean }
  | { kind: "navigate"; id: string; url: string }
  | { kind: "back" | "forward" | "reload" | "close"; id: string };
export interface PreviewBrowserState { id: string; url: string; title: string; back: boolean; forward: boolean; loading: boolean; error: string | null }
