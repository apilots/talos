import type { AppProtocol } from "@talos/sdk-core";

import {
  AppServerClient,
  type AppServerEventHandler,
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
  ): Promise<T>;
}

export interface DesktopTimelineEventSource {
  onEvent(handler: AppServerEventHandler): () => void;
  onMessageQueueChanged?(handler: AppServerMessageQueueChangedHandler): () => void;
  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void;
}

export interface DesktopRendererBridge extends DesktopTimelineEventSource {
  connect?(): Promise<AppProtocol.InitializeResponse>;
  invoke: DesktopTalosInvoke;
  openExternal?(url: string): Promise<void>;
}

export async function getDesktopServerInfo(
  bridge: DesktopRendererBridge,
): Promise<AppProtocol.ServerInfo> {
  if (!bridge.connect) {
    throw new Error("Desktop runtime connect bridge is not available.");
  }
  const response = await bridge.connect();
  return response.server_info;
}

class DesktopRendererTransport implements AppServerTransport {
  constructor(
    private readonly bridge: DesktopRendererBridge,
    private readonly options: ElectronAppServerClientOptions,
  ) {}

  request<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: AppServerRequestOptions,
  ): Promise<T> {
    return this.bridge.invoke<T>(method, params, {
      actorCredentials: options?.actorCredentials ?? this.options.getActorCredentials?.() ?? null,
    });
  }

  onEvent(handler: AppServerEventHandler): () => void {
    return this.bridge.onEvent(handler);
  }

  onMessageQueueChanged(handler: AppServerMessageQueueChangedHandler): () => void {
    return this.bridge.onMessageQueueChanged?.(handler) ?? (() => undefined);
  }

  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void {
    return this.bridge.onSubscriptionTerminated(handler);
  }
}

export class ElectronAppServerClient extends AppServerClient {
  constructor(bridge: DesktopRendererBridge, options: ElectronAppServerClientOptions = {}) {
    super(new DesktopRendererTransport(bridge, options));
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
