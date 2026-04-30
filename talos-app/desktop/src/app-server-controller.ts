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
  invoke<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T>;
  onEvent(handler: (envelope: AppProtocol.ApplicationEventEnvelope) => void): () => void;
  onExit(handler: (status: { code: number | null; signal: NodeJS.Signals | null }) => void): () => void;
  onStderr(handler: (chunk: string) => void): () => void;
}

export interface DesktopAppServerControllerOptions extends AppServerClientOptions {
  bridgeFactory?: (options: AppServerClientOptions) => AppServerBridge;
  onEvent?: (envelope: AppProtocol.ApplicationEventEnvelope) => void;
  onRuntimeState?: (state: TalosRuntimeState) => void;
  restartDelayMs?: number;
}

interface SubscriptionReplayState {
  sessionId: string;
  replayLimit: number | null;
  lastEventId: string | null;
}

export class DesktopAppServerController {
  private bridge?: AppServerBridge;
  private reconnecting?: Promise<void>;
  private closed = false;
  private readonly subscriptions = new Map<string, SubscriptionReplayState>();

  constructor(private readonly options: DesktopAppServerControllerOptions) {}

  async connect(): Promise<AppProtocol.InitializeResponse> {
    this.closed = false;
    this.emitRuntimeState("starting", null);
    const response = await this.ensureBridge().connect();
    this.emitRuntimeState("ready", null);
    return response;
  }

  async invoke<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    if (this.reconnecting) {
      await this.reconnecting;
    }

    const result = await this.ensureBridge().invoke<T>(method, params);
    if (method === "event.subscribe") {
      this.recordSubscription(params as AppProtocol.EventSubscribeParams, result as AppProtocol.EventSubscribeResponse);
    }
    return result;
  }

  async close(): Promise<void> {
    this.closed = true;
    const bridge = this.bridge;
    this.bridge = undefined;
    this.subscriptions.clear();
    await bridge?.close();
    this.emitRuntimeState("stopped", null);
  }

  private ensureBridge(): AppServerBridge {
    if (this.bridge) {
      return this.bridge;
    }

    const bridge = this.createBridge();
    bridge.onEvent((envelope) => this.handleEvent(envelope));
    bridge.onExit((status) => this.handleExit(status));
    bridge.onStderr((chunk) => {
      if (chunk.trim()) {
        this.emitRuntimeState("ready", chunk.trim());
      }
    });
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

  private handleEvent(envelope: AppProtocol.ApplicationEventEnvelope): void {
    const subscription = this.subscriptions.get(envelope.event.session_id);
    if (subscription) {
      subscription.lastEventId = envelope.event_id;
    }
    this.options.onEvent?.(envelope);
  }

  private handleExit(status: { code: number | null; signal: NodeJS.Signals | null }): void {
    this.bridge = undefined;
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
        lastEventId: response.high_water_mark_event_id ?? subscription.lastEventId,
      });
    }
  }

  private recordSubscription(
    params: AppProtocol.EventSubscribeParams,
    response: AppProtocol.EventSubscribeResponse,
  ): void {
    this.subscriptions.set(params.session_id, {
      sessionId: response.session_id,
      replayLimit: params.replay_limit,
      lastEventId: response.high_water_mark_event_id ?? params.since_event_id,
    });
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
