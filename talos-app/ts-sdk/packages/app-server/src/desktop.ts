import type { AppProtocol } from "@talos/sdk-core";

import {
  AppServerClient,
  type AppServerEventHandler,
  type AppServerTransport,
} from "@talos/sdk-core";

export type DesktopTalosMethod = AppProtocol.ClientMethod;

export interface DesktopTalosInvoke {
  <T>(method: DesktopTalosMethod, params: unknown): Promise<T>;
}

export interface DesktopTalosEventSource {
  onEvent(handler: AppServerEventHandler): () => void;
}

export interface DesktopRendererBridge extends DesktopTalosEventSource {
  invoke: DesktopTalosInvoke;
}

class DesktopRendererTransport implements AppServerTransport {
  constructor(private readonly bridge: DesktopRendererBridge) {}

  request<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    return this.bridge.invoke<T>(method, params);
  }

  onEvent(handler: AppServerEventHandler): () => void {
    return this.bridge.onEvent(handler);
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
