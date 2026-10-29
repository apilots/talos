import {createNativeDesktopStream} from "./desktop-stream";
export {createNativeDesktopStream} from "./desktop-stream";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopBrowserBridge } from "./desktop-browser";
export type { DesktopBrowserBridge, DesktopBrowserState, DesktopBrowserTab, DesktopBrowserRequest, DesktopBrowserBounds } from "./desktop-browser";

import {
  AppServerClient,
  unwrapTalosApiResponse,
  type AppServerEventHandler,
  type AppServerActiveTurnChangedHandler,
  type AppServerMessageQueueChangedHandler,
  type AppServerRequestOptions,
  type AppServerSubscriptionTerminatedHandler,
  type AppServerTransport,
} from "@talos/sdk-core";

export type DesktopTalosMethod = AppProtocol.ClientMethod;

export interface DesktopTalosInvoke {
  <T>(
    method: DesktopTalosMethod,
    params: unknown,
    options?: AppServerRequestOptions,
  ): Promise<AppProtocol.ApiResponse<T>>;
}

export interface DesktopTimelineEventSource {
  onEvent(handler: AppServerEventHandler): () => void;
  onActiveTurnChanged?(handler: AppServerActiveTurnChangedHandler): () => void;
  onMessageQueueChanged?(handler: AppServerMessageQueueChangedHandler): () => void;
  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void;
}

export type TalosRuntimeStateKind = "starting" | "ready" | "restarting" | "stopped" | "error";
export interface TalosRuntimeState {
  kind: TalosRuntimeStateKind;
  message: string | null;
  error?: AppProtocol.ApiError | null;
}

export interface DesktopRendererBridge extends DesktopTimelineEventSource {
  preview?: import("./preview-shell").DesktopPreviewBridge;
  browserWorkspace?: DesktopBrowserBridge;
  onDesktopFrame?(handler:(frame:AppProtocol.DesktopMediaFrame)=>void):()=>void;
  onRuntimeState?(handler: (state: TalosRuntimeState) => void): () => void;
  connect?(): Promise<AppProtocol.ApiResponse<AppProtocol.InitializeResponse>>;
  invoke: DesktopTalosInvoke;
  openExternal?(url: string): Promise<void>;
}

export async function getDesktopServerInfo(
  bridge: DesktopRendererBridge,
): Promise<AppProtocol.ServerInfo> {
  if (!bridge.connect) {
    throw new Error("Desktop runtime connect bridge is not available.");
  }
  const response = unwrapTalosApiResponse<AppProtocol.InitializeResponse>(await bridge.connect());
  return response.server_info;
}

class DesktopRendererTransport implements AppServerTransport {
  constructor(
    private readonly bridge: DesktopRendererBridge,
    private readonly options: ElectronAppServerClientOptions,
  ) {}

  async request<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: AppServerRequestOptions,
  ): Promise<T> {
    return unwrapTalosApiResponse<T>(await this.bridge.invoke<T>(method, params, {
      actorCredentials: options?.actorCredentials ?? this.options.getActorCredentials?.() ?? null,
    }));
  }

  onEvent(handler: AppServerEventHandler): () => void {
    return this.bridge.onEvent(handler);
  }

  onActiveTurnChanged(handler: AppServerActiveTurnChangedHandler): () => void { return this.bridge.onActiveTurnChanged?.(handler) ?? (() => undefined); }

  onMessageQueueChanged(handler: AppServerMessageQueueChangedHandler): () => void {
    return this.bridge.onMessageQueueChanged?.(handler) ?? (() => undefined);
  }

  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void {
    return this.bridge.onSubscriptionTerminated(handler);
  }
}

export class ElectronAppServerClient extends AppServerClient {
  constructor(bridge: DesktopRendererBridge, options: ElectronAppServerClientOptions = {}) {
    super(new DesktopRendererTransport(bridge, options), {connect: grant => createNativeDesktopStream(bridge, grant, options.getActorCredentials?.() ?? null)});
  }
}

export interface ElectronAppServerClientOptions {
  getActorCredentials?: () => AppProtocol.ActorCredentials | null | undefined;
}

export function createElectronAppServerClient(
  bridge: DesktopRendererBridge,
  options: ElectronAppServerClientOptions = {},
) {
  return new ElectronAppServerClient(bridge, options);
}

export type { DesktopPreviewBridge, PreviewOpenResource, PreviewOpenTarget, PreviewOpenTargetId, PreviewBrowserBounds, PreviewBrowserOperation, PreviewBrowserState } from "./preview-shell";
