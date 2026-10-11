import type { AppProtocol } from "@talos/sdk-core";

import type {
  AppServerErrorHandler,
  AppServerEventHandler,
  AppServerMessageQueueChangedHandler,
  AppServerRequestOptions,
  AppServerSubscriptionTerminatedHandler,
} from "@talos/sdk-core";
import {
  StdioAppServerClient,
  type AppServerClientOptions,
  type AppServerRequestHandler,
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

  invoke<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: AppServerRequestOptions,
  ): Promise<T> {
    return this.client.transport.request<T>(method, params, options);
  }

  onNotification(handler: (notification: AppProtocol.JsonRpcNotification) => void): () => void {
    return this.client.transport.onNotification(handler);
  }

  onEvent(handler: AppServerEventHandler): () => void {
    return this.client.onEvent(handler);
  }

  onMessageQueueChanged(handler: AppServerMessageQueueChangedHandler): () => void {
    return this.client.onMessageQueueChanged(handler);
  }

  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void {
    return this.client.onSubscriptionTerminated(handler);
  }

  onError(handler: AppServerErrorHandler): () => void {
    return this.client.onError(handler);
  }

  onExit(
    handler: (status: { code: number | null; signal: NodeJS.Signals | null }) => void,
  ): () => void {
    return this.client.transport.onExit(handler);
  }

  onStderr(handler: (chunk: string) => void): () => void {
    return this.client.transport.onStderr(handler);
  }

  onServerRequest(handler: AppServerRequestHandler): () => void {
    return this.client.transport.onServerRequest(handler);
  }
}

export function createDesktopMainAppServerBridge(options: AppServerClientOptions) {
  return new DesktopMainAppServerBridge(new StdioAppServerClient(options));
}
