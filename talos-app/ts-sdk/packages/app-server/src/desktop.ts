import type { AppProtocol } from "@talos/sdk-core";

import {
  AppServerClient,
  type AppServerEventHandler,
  type AppServerSubscriptionTerminatedHandler,
  type AppServerTransport,
} from "@talos/sdk-core";

export type DesktopTalosMethod = AppProtocol.ClientMethod;

export interface DesktopTalosInvoke {
  <T>(method: DesktopTalosMethod, params: unknown): Promise<T>;
}

export interface DesktopTalosEventSource {
  onEvent(handler: AppServerEventHandler): () => void;
  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void;
}

export interface DesktopRendererBridge extends DesktopTalosEventSource {
  connect?(): Promise<AppProtocol.InitializeResponse>;
  invoke: DesktopTalosInvoke;
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
  constructor(private readonly bridge: DesktopRendererBridge) {}

  request<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    return this.bridge.invoke<T>(method, params);
  }

  onEvent(handler: AppServerEventHandler): () => void {
    return this.bridge.onEvent(handler);
  }

  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void {
    return this.bridge.onSubscriptionTerminated(handler);
  }
}

export class ElectronAppServerClient extends AppServerClient {
  constructor(bridge: DesktopRendererBridge) {
    super(new DesktopRendererTransport(bridge));
  }
}

export function createElectronAppServerClient(bridge: DesktopRendererBridge) {
  return new ElectronAppServerClient(bridge);
}
