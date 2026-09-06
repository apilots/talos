import { AppProtocol } from "@talos/sdk-core";
import type { AppServerBridge } from "./app-server-controller";

export class FakeBridge implements AppServerBridge {
  connected = false;
  closed = false;
  readonly requests: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
  private eventHandler?: (event: AppProtocol.TimelineEvent) => void;
  private subscriptionTerminatedHandler?: (notification: AppProtocol.SubscriptionTerminatedNotification) => void;
  private exitHandler?: (status: { code: number | null; signal: NodeJS.Signals | null }) => void;
  private errorHandler?: (error: Error) => void;

  constructor(private readonly subscriptionPrefix = "sub") {}

  connect(): Promise<AppProtocol.InitializeResponse> {
    this.connected = true;
    return Promise.resolve({
      protocol_version: AppProtocol.PROTOCOL_VERSION,
      server_info: {
        name: "fake",
        version: "0.1.5",
        auth_enabled: false,
        cloud_auth_url: null,
      },
    });
  }

  close(): Promise<void> {
    this.closed = true;
    return Promise.resolve();
  }

  invoke<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    this.requests.push({ method, params });
    if (method === "event/subscribe") {
      const request = params as AppProtocol.SubscribeTimelineRequest;
      return Promise.resolve({
        subscriptionId: `${this.subscriptionPrefix}-${this.requests.length}`,
        sessionId: request.sessionId,
        highWaterMarkEventId: request.sinceEventId,
      } satisfies AppProtocol.SubscribeTimelineResponse as T);
    }
    return Promise.resolve({} as T);
  }

  onEvent(handler: (event: AppProtocol.TimelineEvent) => void): () => void {
    this.eventHandler = handler;
    return () => {
      this.eventHandler = undefined;
    };
  }

  onSubscriptionTerminated(handler: (notification: AppProtocol.SubscriptionTerminatedNotification) => void): () => void {
    this.subscriptionTerminatedHandler = handler;
    return () => {
      this.subscriptionTerminatedHandler = undefined;
    };
  }

  onError(handler: (error: Error) => void): () => void {
    this.errorHandler = handler;
    return () => { this.errorHandler = undefined; };
  }

  onExit(handler: (status: { code: number | null; signal: NodeJS.Signals | null }) => void): () => void {
    this.exitHandler = handler;
    return () => {
      this.exitHandler = undefined;
    };
  }

  onStderr(): () => void {
    return () => undefined;
  }

  onServerRequest(): () => void {
    return () => undefined;
  }

  emitEvent(event: AppProtocol.TimelineEvent): void {
    this.eventHandler?.(event);
  }

  emitExit(): void {
    this.exitHandler?.({ code: 1, signal: null });
  }

  emitError(error: Error): void {
    this.errorHandler?.(error);
  }

  emitSubscriptionTerminated(notification: AppProtocol.SubscriptionTerminatedNotification): void {
    this.subscriptionTerminatedHandler?.(notification);
  }
}
