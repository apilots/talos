import type { AppProtocol } from "@talos/sdk-core";

/** Local presentation only. Website targets never receive this product bridge. */
export type DesktopBrowserTab = {
  id: string; title: string; url: string;
  back: boolean; forward: boolean; loading: boolean;
};
export type DesktopBrowserState = {
  sessionId: string; revision: number; tabs: DesktopBrowserTab[];
  revealTabId: string | null; error: string | null;
};
export type DesktopBrowserBounds = { x: number; y: number; width: number; height: number };
export type DesktopBrowserRequest =
  | { kind: "bind"; actorCredentials: AppProtocol.ActorCredentials | null }
  | { kind: "unbind"; sessionId: string }
  | { kind: "new"; sessionId: string }
  | { kind: "present"; sessionId: string; tabId: string | null; bounds: DesktopBrowserBounds | null }
  | { kind: "navigate"; sessionId: string; tabId: string; url: string }
  | { kind: "back" | "forward" | "reload" | "close"; sessionId: string; tabId: string };
export interface DesktopBrowserBridge {
  request(request: DesktopBrowserRequest): Promise<AppProtocol.ApiResponse<DesktopBrowserState | null>>;
  onState(handler: (state: DesktopBrowserState) => void): () => void;
}
