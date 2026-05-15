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
  onEvent(handler: (event: AppProtocol.TimelineEventEnvelope) => void): () => void;
  onSubscriptionTerminated(handler: (envelope: AppProtocol.SubscriptionTerminatedEnvelope) => void): () => void;
  onError(handler: (error: Error) => void): () => void;
  onExit(handler: (status: { code: number | null; signal: NodeJS.Signals | null }) => void): () => void;
  onStderr(handler: (chunk: string) => void): () => void;
  onServerRequest(
    handler: (request: AppProtocol.JsonRpcRequest) => Promise<unknown> | unknown,
  ): () => void;
}

export interface DesktopAppServerControllerOptions extends AppServerClientOptions {
  bridgeFactory?: (options: AppServerClientOptions) => AppServerBridge;
  onEvent?: (event: AppProtocol.TimelineEventEnvelope) => void;
  onSubscriptionTerminated?: (envelope: AppProtocol.SubscriptionTerminatedEnvelope) => void;
  onRuntimeState?: (state: TalosRuntimeState) => void;
  onServerRequest?: (request: AppProtocol.JsonRpcRequest) => Promise<unknown> | unknown;
  restartDelayMs?: number;
}

interface SubscriptionReplayState {
  sessionId: string;
  subscriptionId: string;
  replayLimit: number | null;
  lastEventId: string | null;
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

    const result = await this.ensureBridge().invoke<T>(method, params, options);
    if (method === "event.subscribe") {
      this.recordSubscription(params as AppProtocol.EventSubscribeParams, result as AppProtocol.EventSubscribeResponse);
    }
    if (method === "event.unsubscribe") {
      this.forgetSubscription((params as AppProtocol.EventUnsubscribeParams).subscription_id);
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
    bridge.onEvent((envelope) => this.handleEvent(envelope));
    bridge.onSubscriptionTerminated((envelope) => this.handleSubscriptionTerminated(envelope));
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

  private handleEvent(event: AppProtocol.TimelineEventEnvelope): void {
    const subscription = this.subscriptions.get(event.session_id);
    if (subscription) {
      subscription.lastEventId = event.event_id;
    }
    this.options.onEvent?.(event);
  }

  private handleSubscriptionTerminated(envelope: AppProtocol.SubscriptionTerminatedEnvelope): void {
    this.forgetSubscription(envelope.subscription_id);
    this.options.onSubscriptionTerminated?.(envelope);
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
      const response = await this.ensureBridge().invoke<AppProtocol.EventSubscribeResponse>(
        "event.subscribe",
        {
          session_id: subscription.sessionId,
          since_event_id: subscription.lastEventId,
          replay_limit: subscription.replayLimit,
        } satisfies AppProtocol.EventSubscribeParams,
      );
      this.subscriptions.set(subscription.sessionId, {
        ...subscription,
        subscriptionId: response.subscription_id,
        lastEventId: response.high_water_mark_event_id ?? subscription.lastEventId,
      });
      this.sessionsBySubscriptionId.delete(subscription.subscriptionId);
      this.sessionsBySubscriptionId.set(response.subscription_id, subscription.sessionId);
    }
  }

  private recordSubscription(
    params: AppProtocol.EventSubscribeParams,
    response: AppProtocol.EventSubscribeResponse,
  ): void {
    const previous = this.subscriptions.get(params.session_id);
    if (previous) {
      this.sessionsBySubscriptionId.delete(previous.subscriptionId);
    }
    this.subscriptions.set(params.session_id, {
      sessionId: response.session_id,
      subscriptionId: response.subscription_id,
      replayLimit: params.replay_limit,
      lastEventId: response.high_water_mark_event_id ?? params.since_event_id,
    });
    this.sessionsBySubscriptionId.set(response.subscription_id, params.session_id);
  }

  private forgetSubscription(subscriptionId: string): void {
    const sessionId = this.sessionsBySubscriptionId.get(subscriptionId);
    if (!sessionId) {
      return;
    }
    this.sessionsBySubscriptionId.delete(subscriptionId);
    this.subscriptions.delete(sessionId);
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
