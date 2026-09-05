import type { AppProtocol } from "@talos/sdk-core";
import {
  createDesktopMainAppServerBridge,
  type AppServerClientOptions,
  type DesktopMainAppServerBridge,
} from "@talos/sdk-app-server/desktop-main";

import type { TalosRuntimeState } from "./ipc";

export interface AppServerBridge {
  connect(): Promise<AppProtocol.InitializeResponse>;
  close(): Promise<void>;
  invoke<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: { actorCredentials?: AppProtocol.ActorCredentials | null },
  ): Promise<T>;
  onEvent(handler: (event: AppProtocol.TimelineEvent) => void): () => void;
  onSubscriptionTerminated(handler: (notification: AppProtocol.SubscriptionTerminatedNotification) => void): () => void;
  onError(handler: (error: Error) => void): () => void;
  onExit(handler: (status: { code: number | null; signal: NodeJS.Signals | null }) => void): () => void;
  onStderr(handler: (chunk: string) => void): () => void;
  onServerRequest(
    handler: (request: AppProtocol.JsonRpcRequest) => Promise<unknown> | unknown,
  ): () => void;
}

export interface DesktopAppServerControllerOptions extends AppServerClientOptions {
  bridgeFactory?: (options: AppServerClientOptions) => AppServerBridge;
  onEvent?: (event: AppProtocol.TimelineEvent) => void;
  onSubscriptionTerminated?: (notification: AppProtocol.SubscriptionTerminatedNotification) => void;
  onRuntimeState?: (state: TalosRuntimeState) => void;
  onServerRequest?: (request: AppProtocol.JsonRpcRequest) => Promise<unknown> | unknown;
  restartDelayMs?: number;
}

interface SubscriptionReplayState {
  sessionId: string;
  turnId: string | null;
  subscriptionId: string;
  replayLimit: number | null;
  lastEventId: string | null;
  closeWhenIdle: boolean | null;
  closeOnTerminal: boolean | null;
}

export class DesktopAppServerController {
  private bridge?: AppServerBridge;
  private initializeResponse?: AppProtocol.InitializeResponse;
  private reconnecting?: Promise<void>;
  private closed = false;
  private readonly subscriptions = new Map<string, SubscriptionReplayState>();
  private readonly sessionsBySubscriptionId = new Map<string, string>();

  constructor(private readonly options: DesktopAppServerControllerOptions) {}

  async connect(): Promise<AppProtocol.InitializeResponse> {
    if (this.initializeResponse) {
      return this.initializeResponse;
    }
    this.closed = false;
    this.emitRuntimeState("starting", null);
    const response = await this.ensureBridge().connect();
    this.initializeResponse = response;
    this.emitRuntimeState("ready", null);
    return response;
  }

  async invoke<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: { actorCredentials?: AppProtocol.ActorCredentials | null },
  ): Promise<T> {
    if (this.reconnecting) {
      await this.reconnecting;
    }

    const effectiveParams = method === "event/unsubscribe"
      ? this.resolveUnsubscribeParams(params as AppProtocol.EventUnsubscribeRequest)
      : params;
    const result = await this.ensureBridge().invoke<T>(method, effectiveParams, options);
    if (method === "event/subscribe") {
      this.recordSubscription(params as AppProtocol.SubscribeTimelineRequest, result as AppProtocol.SubscribeTimelineResponse);
    }
    if (method === "event/unsubscribe") {
      this.forgetSubscription((params as AppProtocol.EventUnsubscribeRequest).subscriptionId);
    }
    return result;
  }

  async close(): Promise<void> {
    this.closed = true;
    const bridge = this.bridge;
    this.bridge = undefined;
    this.initializeResponse = undefined;
    this.subscriptions.clear();
    this.sessionsBySubscriptionId.clear();
    await bridge?.close();
    this.emitRuntimeState("stopped", null);
  }

  private ensureBridge(): AppServerBridge {
    if (this.bridge) {
      return this.bridge;
    }

    const bridge = this.createBridge();
    bridge.onEvent((event) => this.handleEvent(event));
    bridge.onSubscriptionTerminated((notification) => this.handleSubscriptionTerminated(notification));
    bridge.onError((error) => this.handleError(error));
    bridge.onExit((status) => this.handleExit(status));
    bridge.onStderr((chunk) => {
      if (chunk.trim()) {
        this.emitRuntimeState("ready", chunk.trim());
      }
    });
    bridge.onServerRequest((request) => this.handleServerRequest(request));
    this.bridge = bridge;
    return bridge;
  }

  private createBridge(): AppServerBridge {
    const bridgeFactory =
      this.options.bridgeFactory ??
      ((options: AppServerClientOptions): DesktopMainAppServerBridge =>
        createDesktopMainAppServerBridge(options));
    return bridgeFactory(this.options);
  }

  private handleEvent(event: AppProtocol.TimelineEvent): void {
    const subscription = this.subscriptions.get(event.event.identity.sessionId);
    if (subscription) {
      subscription.lastEventId = event.eventId;
    }
    this.options.onEvent?.(event);
  }

  private handleSubscriptionTerminated(notification: AppProtocol.SubscriptionTerminatedNotification): void {
    this.forgetSubscription(notification.subscriptionId);
    this.options.onSubscriptionTerminated?.(notification);
  }

  private handleError(error: Error): void {
    this.emitRuntimeState("error", error.message);
  }

  private handleServerRequest(request: AppProtocol.JsonRpcRequest): Promise<unknown> | unknown {
    if (!this.options.onServerRequest) {
      throw new Error(`Unsupported desktop server request: ${request.method}`);
    }
    return this.options.onServerRequest(request);
  }

  private handleExit(status: { code: number | null; signal: NodeJS.Signals | null }): void {
    this.bridge = undefined;
    this.initializeResponse = undefined;
    if (this.closed) {
      return;
    }

    const message = `app-server exited: code=${status.code ?? "null"} signal=${status.signal ?? "null"}`;
    this.emitRuntimeState("restarting", message);
    this.reconnecting = this.restartAndReplay().finally(() => {
      this.reconnecting = undefined;
    });
  }

  private async restartAndReplay(): Promise<void> {
    await delay(this.options.restartDelayMs ?? 300);
    try {
      await this.connect();
      await this.replaySubscriptions();
    } catch (error) {
      this.emitRuntimeState("error", errorMessage(error));
      throw error;
    }
  }

  private async replaySubscriptions(): Promise<void> {
    const subscriptions = [...this.subscriptions.values()];
    for (const subscription of subscriptions) {
      const response = await this.ensureBridge().invoke<AppProtocol.SubscribeTimelineResponse>(
        "event/subscribe",
        {
          sessionId: subscription.sessionId,
          turnId: subscription.turnId,
          sinceEventId: subscription.lastEventId,
          replayLimit: subscription.replayLimit,
          closeWhenIdle: subscription.closeWhenIdle,
          closeOnTerminal: subscription.closeOnTerminal,
        } satisfies AppProtocol.SubscribeTimelineRequest,
      );
      this.subscriptions.set(subscription.sessionId, {
        ...subscription,
        subscriptionId: response.subscriptionId,
        lastEventId: response.highWaterMarkEventId ?? subscription.lastEventId,
      });
      this.sessionsBySubscriptionId.set(subscription.subscriptionId, subscription.sessionId);
      this.sessionsBySubscriptionId.set(response.subscriptionId, subscription.sessionId);
    }
  }

  private recordSubscription(
    params: AppProtocol.SubscribeTimelineRequest,
    response: AppProtocol.SubscribeTimelineResponse,
  ): void {
    const previous = this.subscriptions.get(params.sessionId);
    if (previous) {
      for (const [subscriptionId, sessionId] of this.sessionsBySubscriptionId) {
        if (sessionId === params.sessionId) {
          this.sessionsBySubscriptionId.delete(subscriptionId);
        }
      }
    }
    this.subscriptions.set(params.sessionId, {
      sessionId: response.sessionId,
      turnId: params.turnId ?? null,
      subscriptionId: response.subscriptionId,
      replayLimit: params.replayLimit,
      lastEventId: response.highWaterMarkEventId ?? params.sinceEventId,
      closeWhenIdle: params.closeWhenIdle ?? null,
      closeOnTerminal: params.closeOnTerminal ?? null,
    });
    this.sessionsBySubscriptionId.set(response.subscriptionId, params.sessionId);
  }

  private forgetSubscription(subscriptionId: string): void {
    const sessionId = this.sessionsBySubscriptionId.get(subscriptionId);
    if (!sessionId) {
      return;
    }
    for (const [knownSubscriptionId, knownSessionId] of this.sessionsBySubscriptionId) {
      if (knownSessionId === sessionId) {
        this.sessionsBySubscriptionId.delete(knownSubscriptionId);
      }
    }
    this.subscriptions.delete(sessionId);
  }

  private resolveUnsubscribeParams(
    params: AppProtocol.EventUnsubscribeRequest,
  ): AppProtocol.EventUnsubscribeRequest {
    const sessionId = this.sessionsBySubscriptionId.get(params.subscriptionId);
    const currentSubscriptionId = sessionId
      ? this.subscriptions.get(sessionId)?.subscriptionId
      : undefined;
    return {
      subscriptionId: currentSubscriptionId ?? params.subscriptionId,
    };
  }

  private emitRuntimeState(kind: TalosRuntimeState["kind"], message: string | null): void {
    this.options.onRuntimeState?.({ kind, message });
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
