import { AppProtocol, TalosApiError, decodeApiError } from "@talos/sdk-core";
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
  onNotification(handler: (notification: AppProtocol.JsonRpcNotification) => void): () => void;
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
  onActiveTurnChanged?: (notification: AppProtocol.SessionActiveTurnChangedNotification) => void;
  onMessageQueueChanged?: (notification: AppProtocol.MessageQueueChangedNotification) => void;
  onEvent?: (event: AppProtocol.TimelineEvent) => void;
  onSubscriptionTerminated?: (notification: AppProtocol.SubscriptionTerminatedNotification) => void;
  onRuntimeState?: (state: TalosRuntimeState) => void;
  onServerRequest?: (request: AppProtocol.JsonRpcRequest) => Promise<unknown> | unknown;
  restartDelayMs?: number;
  /** Enable host enrollment only for the packaged Desktop composition. */
  desktopHost?: boolean;
}

interface SubscriptionReplayState {
  userId: string | null;
  sessionId: string;
  turnId: string | null;
  subscriptionId: string | null;
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
  private subscriptionEpoch = 0;
  private readonly subscriptions = new Map<string, SubscriptionReplayState>();
  private readonly sessionsBySubscriptionId = new Map<string, string>();

  constructor(private readonly options: DesktopAppServerControllerOptions) {}

  async connect(): Promise<AppProtocol.InitializeResponse> {
    if (this.initializeResponse) {
      return this.initializeResponse;
    }
    this.closed = false;
    this.emitRuntimeState("starting", null);
    const bridge = this.ensureBridge();
    const response = await bridge.connect();
    if (this.closed || this.bridge !== bridge) return response;
    if (this.options.desktopHost && response.server_info.auth_enabled) {
      const session = await bridge.invoke<AppProtocol.AuthSessionReadResponse>("auth/session/read", {});
      if (this.closed || this.bridge !== bridge) return response;
      this.initializeResponse = response;
      this.emitRuntimeState("ready", null);
      if (session.session) { await this.enrollWorker(session.session.access_token); }
      return response;
    }
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

    if (method === "auth/logout" || method === "auth/session/clear") {
      this.clearSubscriptions();
    }
    const bridge = this.ensureBridge();
    const epoch = this.subscriptionEpoch;
    const userId = method === "event/subscribe" && this.initializeResponse?.server_info.auth_enabled
      ? (await bridge.invoke<AppProtocol.UserCurrentResponse>("user/current", {}, options)).user.user_id
      : null;

    const effectiveParams = method === "event/unsubscribe"
      ? this.resolveUnsubscribeParams(params as AppProtocol.EventUnsubscribeRequest)
      : params;
    const previous = method === "event/subscribe" ? this.subscriptions.get((params as AppProtocol.SubscribeTimelineRequest).sessionId) : undefined;
    const subscription = method === "event/subscribe" && epoch === this.subscriptionEpoch && this.bridge === bridge && !this.closed
      ? this.beginSubscription(params as AppProtocol.SubscribeTimelineRequest, userId)
      : undefined;
    let result: T;
    try {
      result = await bridge.invoke<T>(method, effectiveParams, options);
    } catch (error) {
      if (subscription && this.subscriptions.get(subscription.sessionId) === subscription) {
        if (previous) this.subscriptions.set(subscription.sessionId, previous);
        else this.subscriptions.delete(subscription.sessionId);
      }
      throw error;
    }
    if (method === "auth/logout" || method === "auth/session/clear") this.clearSubscriptions();
    if (method === "auth/login/status") {
      const login = result as AppProtocol.CloudLoginStatusResponse;
      if (login.status === "authenticated") this.clearSubscriptions();
    }
    if (method === "auth/session/read") {
      this.retainAccountSubscriptions((result as AppProtocol.AuthSessionReadResponse).session?.user.user_id ?? null);
    }
    if (this.options.desktopHost && this.initializeResponse?.server_info.auth_enabled) {
      const login = method === "auth/login/status" ? result as AppProtocol.CloudLoginStatusResponse : null;
      const session = method === "auth/session/read" ? (result as AppProtocol.AuthSessionReadResponse).session : null;
      const token = login?.status === "authenticated" ? login.access_token : session?.access_token;
      if (token) await this.enrollWorker(token);
    }
    if (method === "desktop/workerEnroll") this.emitRuntimeState("ready", null);
    if (subscription && this.subscriptions.get(subscription.sessionId) === subscription) {
      for (const [subscriptionId, sessionId] of this.sessionsBySubscriptionId) {
        if (sessionId === subscription.sessionId) this.sessionsBySubscriptionId.delete(subscriptionId);
      }
      subscription.subscriptionId = (result as AppProtocol.SubscribeTimelineResponse).subscriptionId;
      this.sessionsBySubscriptionId.set(subscription.subscriptionId, subscription.sessionId);
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
    this.clearSubscriptions();
    await bridge?.close();
    this.emitRuntimeState("stopped", null);
  }

  /** Enrollment is independent of successful login; report its failure without rewriting auth. */
  private async enrollWorker(token: string): Promise<void> {
    try {
      await this.ensureBridge().invoke("sync/status", null, {
        actorCredentials: { credential: { type: "existing_session_token", token } },
      });
      this.emitRuntimeState("ready", null);
    } catch (error) {
      if (!(error instanceof TalosApiError)) { console.error("Desktop Worker enrollment failed", error); }
      this.options.onRuntimeState?.({
        kind: "error",
        message: "Worker registration failed. Recover or reconnect this computer to retry.",
        error: error instanceof TalosApiError ? error.apiError : {
          code: AppProtocol.ErrorCode.InternalError,
          message: "Worker registration failed; technical details are available in the desktop log.",
          localization: null,
        },
      });
    }
  }

  private ensureBridge(): AppServerBridge {
    if (this.bridge) {
      return this.bridge;
    }

    const bridge = this.createBridge();
    bridge.onNotification((notification) => {
      if (this.bridge !== bridge) return;
      if (notification.method === "session/activeTurnChanged") { this.options.onActiveTurnChanged?.(notification.params as AppProtocol.SessionActiveTurnChangedNotification); return; }
      if (notification.method === "messageQueue/changed") { this.options.onMessageQueueChanged?.(notification.params as AppProtocol.MessageQueueChangedNotification); return; }
      if (notification.method === "desktop/worker/ready") { this.emitRuntimeState("ready", null); return; }
      if (notification.method !== "desktop/worker/failed") return;
      try {
        this.options.onRuntimeState?.({ kind: "error", message: null, error: decodeApiError(notification.params) });
      } catch (error) { this.handleError(error); }
    });
    bridge.onEvent((event) => { if (this.bridge === bridge) this.handleEvent(event); });
    bridge.onSubscriptionTerminated((notification) => { if (this.bridge === bridge) this.handleSubscriptionTerminated(notification); });
    bridge.onError((error) => { if (this.bridge === bridge) this.handleError(error); });
    bridge.onExit((status) => { if (this.bridge === bridge) this.handleExit(status); });
    bridge.onStderr((chunk) => {
      if (chunk.trim()) {
        console.error("App-server diagnostics:", chunk.trim());
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
    const subscription = this.subscriptions.get(event.event.data.identity.sessionId);
    if (!subscription) return;
    subscription.lastEventId = event.eventId;
    this.options.onEvent?.(event);
  }

  private handleSubscriptionTerminated(notification: AppProtocol.SubscriptionTerminatedNotification): void {
    if (this.subscriptions.get(notification.sessionId)?.subscriptionId === null) this.subscriptions.delete(notification.sessionId);
    this.forgetSubscription(notification.subscriptionId);
    this.options.onSubscriptionTerminated?.(notification);
  }

  private handleError(error: unknown): void {
    console.error("App-server transport failed:", error);
    const message = "The app-server event stream failed. Please reconnect.";
    this.options.onRuntimeState?.({ kind: "error", message, error: error instanceof TalosApiError ? error.apiError : null });
    for (const subscription of [...this.subscriptions.values()]) {
      if (subscription.subscriptionId === null) { this.subscriptions.delete(subscription.sessionId); continue; }
      this.handleSubscriptionTerminated({
        subscriptionId: subscription.subscriptionId,
        sessionId: subscription.sessionId,
        reason: "interrupted",
        message,
      });
    }
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
    // Reconnect reports and terminates subscriptions even without an awaiting request.
    void this.reconnecting.catch(() => undefined);
  }

  private async restartAndReplay(): Promise<void> {
    await delay(this.options.restartDelayMs ?? 300);
    if (this.closed) return;
    try {
      await this.connect();
      if (this.closed) return;
      await this.replaySubscriptions();
    } catch (error) {
      if (this.closed) return;
      this.handleError(error);
      throw error;
    }
  }

  private async replaySubscriptions(): Promise<void> {
    const subscriptions = [...this.subscriptions.values()];
    if (!subscriptions.length) return;
    const bridge = this.ensureBridge();
    const session = this.initializeResponse?.server_info.auth_enabled
      ? (await bridge.invoke<AppProtocol.AuthSessionReadResponse>("auth/session/read", {})).session
      : null;
    for (const subscription of subscriptions) {
      if (subscription.subscriptionId === null) continue;
      if (this.closed || this.bridge !== bridge || this.subscriptions.get(subscription.sessionId) !== subscription) return;
      if (subscription.userId !== (session?.user.user_id ?? null)) {
        throw new TalosApiError({ code: AppProtocol.ErrorCode.Unauthenticated, message: "Sign in with the subscription's account to reconnect.", localization: null });
      }
      const response = await bridge.invoke<AppProtocol.SubscribeTimelineResponse>(
        "event/subscribe",
        {
          sessionId: subscription.sessionId,
          turnId: subscription.turnId,
          sinceEventId: subscription.lastEventId,
          replayLimit: subscription.replayLimit,
          closeWhenIdle: subscription.closeWhenIdle,
          closeOnTerminal: subscription.closeOnTerminal,
        } satisfies AppProtocol.SubscribeTimelineRequest,
        { actorCredentials: session ? { credential: { type: "existing_session_token", token: session.access_token } } : null },
      );
      if (this.closed || this.bridge !== bridge || this.subscriptions.get(subscription.sessionId) !== subscription) return;
      this.sessionsBySubscriptionId.set(subscription.subscriptionId, subscription.sessionId);
      subscription.subscriptionId = response.subscriptionId;
      this.sessionsBySubscriptionId.set(response.subscriptionId, subscription.sessionId);
    }
  }

  private beginSubscription(
    params: AppProtocol.SubscribeTimelineRequest,
    userId: string | null,
  ): SubscriptionReplayState {
    const subscription: SubscriptionReplayState = {
      userId,
      sessionId: params.sessionId,
      turnId: params.turnId ?? null,
      subscriptionId: null,
      replayLimit: params.replayLimit,
      lastEventId: params.sinceEventId,
      closeWhenIdle: params.closeWhenIdle ?? null,
      closeOnTerminal: params.closeOnTerminal ?? null,
    };
    this.subscriptions.set(params.sessionId, subscription);
    return subscription;
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

  private clearSubscriptions(): void {
    this.subscriptionEpoch += 1;
    const subscriptions = [...this.subscriptions.values()];
    this.subscriptions.clear();
    this.sessionsBySubscriptionId.clear();
    for (const subscription of subscriptions) {
      if (subscription.subscriptionId !== null) this.options.onSubscriptionTerminated?.({ subscriptionId: subscription.subscriptionId, sessionId: subscription.sessionId, reason: "interrupted", message: "The subscription was closed because the desktop account or lifecycle changed." });
    }
  }

  private retainAccountSubscriptions(userId: string | null): void {
    if ([...this.subscriptions.values()].some((subscription) => subscription.userId !== userId)) this.clearSubscriptions();
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
