import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it, vi } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import { createElectronAppServerClient } from "@talos/sdk-app-server/desktop";

import type { AppServerBridge } from "./app-server-controller";
import { DesktopAppServerController } from "./app-server-controller";
import type { TalosRuntimeState } from "./ipc";

class FakeBridge implements AppServerBridge {
  connected = false;
  closed = false;
  readonly requests: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
  private eventHandler?: (event: AppProtocol.TimelineEvent) => void;
  private subscriptionTerminatedHandler?: (notification: AppProtocol.SubscriptionTerminatedNotification) => void;
  private exitHandler?: (status: { code: number | null; signal: NodeJS.Signals | null }) => void;

  constructor(private readonly subscriptionPrefix = "sub") {}

  connect(): Promise<AppProtocol.InitializeResponse> {
    this.connected = true;
    return Promise.resolve({
      protocol_version: { major: 0, minor: 3 },
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

  onError(): () => void {
    return () => undefined;
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

  emitSubscriptionTerminated(notification: AppProtocol.SubscriptionTerminatedNotification): void {
    this.subscriptionTerminatedHandler?.(notification);
  }
}

describe("DesktopAppServerController", () => {
  it("restarts app-server and replays subscriptions from the last seen event", async () => {
    vi.useFakeTimers();
    const bridges: FakeBridge[] = [];
    const controller = new DesktopAppServerController({
      restartDelayMs: 10,
      bridgeFactory: () => {
        const bridge = new FakeBridge(`bridge-${bridges.length}`);
        bridges.push(bridge);
        return bridge;
      },
    });

    await controller.connect();
    await controller.invoke("event/subscribe", {
      sessionId: "session-1",
      turnId: null,
      sinceEventId: null,
      replayLimit: 50,
      closeWhenIdle: true,
      closeOnTerminal: true,
    } satisfies AppProtocol.SubscribeTimelineRequest);

    bridges[0]?.emitEvent(createTimelineEvent("event-7", "session-1", "turnStarted"));
    bridges[0]?.emitExit();

    await vi.advanceTimersByTimeAsync(10);

    expect(bridges).toHaveLength(2);
    expect(bridges[1]?.connected).toBe(true);
    expect(bridges[1]?.requests).toEqual([
      {
        method: "event/subscribe",
        params: {
          sessionId: "session-1",
          turnId: null,
          sinceEventId: "event-7",
          replayLimit: 50,
          closeWhenIdle: true,
          closeOnTerminal: true,
        },
      },
    ]);
    vi.useRealTimers();
  });

  it("does not replay renderer subscriptions after they are unsubscribed", async () => {
    vi.useFakeTimers();
    const bridges: FakeBridge[] = [];
    const controller = new DesktopAppServerController({
      restartDelayMs: 10,
      bridgeFactory: () => {
        const bridge = new FakeBridge();
        bridges.push(bridge);
        return bridge;
      },
    });

    await controller.connect();
    const response = await controller.invoke<AppProtocol.SubscribeTimelineResponse>("event/subscribe", {
      sessionId: "session-1",
      turnId: null,
      sinceEventId: null,
      replayLimit: 50,
      closeWhenIdle: null,
      closeOnTerminal: null,
    } satisfies AppProtocol.SubscribeTimelineRequest);
    await controller.invoke("event/unsubscribe", {
      subscriptionId: response.subscriptionId,
    } satisfies AppProtocol.EventUnsubscribeRequest);

    bridges[0]?.emitExit();
    await vi.advanceTimersByTimeAsync(10);

    expect(bridges).toHaveLength(2);
    expect(bridges[1]?.requests).toEqual([]);
    vi.useRealTimers();
  });

  it("recovers the renderer app-server stream after a child process exit", async () => {
    vi.useFakeTimers();
    const bridges: FakeBridge[] = [];
    const lifecycle: TalosRuntimeState[] = [];
    const rendererHandlers = new Set<(event: AppProtocol.TimelineEvent) => void>();
    const rendererEvents: AppProtocol.TimelineEvent[] = [];
    const controller = new DesktopAppServerController({
      restartDelayMs: 10,
      bridgeFactory: () => {
        const bridge = new FakeBridge();
        bridges.push(bridge);
        return bridge;
      },
      onEvent: (event) => {
        rendererEvents.push(event);
        for (const handler of rendererHandlers) {
          handler(event);
        }
      },
      onRuntimeState: (state) => lifecycle.push(state),
    });
    const rendererClient = createElectronAppServerClient({
      invoke: (method, params) => controller.invoke(method, params),
      onEvent: (handler) => {
        rendererHandlers.add(handler);
        return () => {
          rendererHandlers.delete(handler);
        };
      },
      onSubscriptionTerminated: () => () => undefined,
    });
    const sdkEvents: AppProtocol.TimelineEvent[] = [];
    const unsubscribe = rendererClient.onEvent((event) => sdkEvents.push(event));

    try {
      await controller.connect();
      await rendererClient.subscribeEvents({
        sessionId: "session-1",
        turnId: null,
        sinceEventId: null,
        replayLimit: 50,
        closeWhenIdle: null,
        closeOnTerminal: null,
      });

      bridges[0]?.emitEvent(createTimelineEvent("event-7", "session-1", "turnStarted"));
      bridges[0]?.emitExit();

      await vi.advanceTimersByTimeAsync(10);
      bridges[1]?.emitEvent(
        createTimelineEvent("event-8", "session-1", "turnCompleted"),
      );

      expect(lifecycle.map((state) => state.kind)).toEqual([
        "starting",
        "ready",
        "restarting",
        "starting",
        "ready",
      ]);
      expect(bridges).toHaveLength(2);
      expect(bridges[1]?.requests).toEqual([
        {
          method: "event/subscribe",
          params: {
            sessionId: "session-1",
            turnId: null,
            sinceEventId: "event-7",
            replayLimit: 50,
            closeWhenIdle: null,
            closeOnTerminal: null,
          },
        },
      ]);
      expect(sdkEvents.map((event) => event.eventId)).toEqual(["event-7", "event-8"]);
      expect(rendererEvents.map((event) => event.eventId)).toEqual(["event-7", "event-8"]);
    } finally {
      unsubscribe();
      await controller.close();
      writeDesktopSmokeArtifact({
        lifecycle,
        bridgeRequests: bridges.map((bridge) => bridge.requests),
        rendererEventIds: eventIds(rendererEvents),
        sdkEventIds: eventIds(sdkEvents),
      });
      vi.useRealTimers();
    }
  });
});

function createTimelineEvent(
  eventId: string,
  sessionId: string,
  eventType: "turnStarted" | "turnCompleted",
): AppProtocol.TimelineEvent {
  return {
    eventSeq: 1,
    eventId,
    persistedAt: 1,
    event: {
      identity: { sessionId, turnId: "turn-1" },
      sequence: 0,
      occurredAt: 1,
      payload:
        eventType === "turnStarted"
          ? {
              type: "turnStarted",
              data: {},
            }
          : {
              type: "turnCompleted",
              data: { stopReason: "endTurn" },
            },
    },
  };
}

interface DesktopSmokeArtifact {
  lifecycle: TalosRuntimeState[];
  bridgeRequests: Array<Array<{ method: AppProtocol.ClientMethod; params: unknown }>>;
  rendererEventIds: string[];
  sdkEventIds: string[];
}

function eventIds(events: AppProtocol.TimelineEvent[]): string[] {
  return events.map((event) => event.eventId);
}

function writeDesktopSmokeArtifact(artifact: DesktopSmokeArtifact): void {
  const outputDir = path.resolve(process.cwd(), "../../test-results/desktop");
  mkdirSync(outputDir, { recursive: true });
  writeFileSync(
    path.join(outputDir, "app-server-crash-recovery-smoke.json"),
    `${JSON.stringify(artifact, null, 2)}\n`,
  );
}
