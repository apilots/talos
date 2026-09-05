import { describe, expect, it, vi } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";

import type { AppServerBridge } from "./app-server-controller";
import { DesktopAppServerController } from "./app-server-controller";

class RecoveryBridge implements AppServerBridge {
  readonly requests: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
  private exitHandler?: (status: { code: number | null; signal: NodeJS.Signals | null }) => void;

  constructor(private readonly subscriptionPrefix: string) {}

  connect(): Promise<AppProtocol.InitializeResponse> {
    return Promise.resolve({
      protocol_version: { major: 0, minor: 3 },
      server_info: { name: "fake", version: "0.1.5", auth_enabled: false, cloud_auth_url: null },
    });
  }

  close(): Promise<void> {
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
      } as T);
    }
    return Promise.resolve({} as T);
  }

  onEvent(): () => void {
    return () => undefined;
  }

  onSubscriptionTerminated(): () => void {
    return () => undefined;
  }

  onError(): () => void {
    return () => undefined;
  }

  onExit(handler: (status: { code: number | null; signal: NodeJS.Signals | null }) => void): () => void {
    this.exitHandler = handler;
    return () => undefined;
  }

  onStderr(): () => void {
    return () => undefined;
  }

  onServerRequest(): () => void {
    return () => undefined;
  }

  emitExit(): void {
    this.exitHandler?.({ code: 1, signal: null });
  }
}

describe("DesktopAppServerController subscription recovery", () => {
  it("maps renderer unsubscribe requests to the replayed subscription id", async () => {
    vi.useFakeTimers();
    const bridges: RecoveryBridge[] = [];
    const controller = new DesktopAppServerController({
      restartDelayMs: 10,
      bridgeFactory: () => {
        const bridge = new RecoveryBridge(`bridge-${bridges.length}`);
        bridges.push(bridge);
        return bridge;
      },
    });

    try {
      await controller.connect();
      const subscription = await controller.invoke<AppProtocol.SubscribeTimelineResponse>("event/subscribe", {
        sessionId: "session-1",
        turnId: null,
        sinceEventId: null,
        replayLimit: 50,
        closeWhenIdle: true,
        closeOnTerminal: true,
      });

      bridges[0]?.emitExit();
      await vi.advanceTimersByTimeAsync(10);
      await controller.invoke("event/unsubscribe", { subscriptionId: subscription.subscriptionId });

      expect(bridges[1]?.requests.at(-1)).toEqual({
        method: "event/unsubscribe",
        params: { subscriptionId: "bridge-1-1" },
      });
    } finally {
      vi.useRealTimers();
    }
  });
});
