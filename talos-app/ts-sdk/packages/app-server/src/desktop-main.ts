import type { AppProtocol } from "@talos/sdk-core";

import type { AppServerEventHandler } from "@talos/sdk-core";
import {
  StdioAppServerClient,
  type AppServerClientOptions,
} from "./stdio";

export type { AppServerClientOptions } from "./stdio";

export class DesktopMainAppServerBridge {
  constructor(readonly client: StdioAppServerClient) {}

  connect(): Promise<AppProtocol.InitializeResponse> {
    return this.client.connect();
  }

  close(): Promise<void> {
    return this.client.close();
  }

  invoke<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    return this.client.transport.request<T>(method, params);
  }

  onEvent(handler: AppServerEventHandler): () => void {
    return this.client.onEvent(handler);
  }

  onExit(
    handler: (status: { code: number | null; signal: NodeJS.Signals | null }) => void,
  ): () => void {
    return this.client.transport.onExit(handler);
  }

  onStderr(handler: (chunk: string) => void): () => void {
    return this.client.transport.onStderr(handler);
  }
}

export function createDesktopMainAppServerBridge(options: AppServerClientOptions) {
  return new DesktopMainAppServerBridge(new StdioAppServerClient(options));
}
