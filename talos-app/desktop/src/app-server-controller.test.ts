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
  private eventHandler?: (envelope: AppProtocol.TimelineEventEnvelope) => void;
  private subscriptionTerminatedHandler?: (envelope: AppProtocol.SubscriptionTerminatedEnvelope) => void;
  private exitHandler?: (status: { code: number | null; signal: NodeJS.Signals | null }) => void;

  connect(): Promise<AppProtocol.InitializeResponse> {
    this.connected = true;
    return Promise.resolve({
      protocol_version: { major: 0, minor: 3 },
      server_info: {
        name: "fake",
        version: "0.1.2",
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
    if (method === "event.subscribe") {
      const request = params as AppProtocol.SubscribeTimelineRequest;
      return Promise.resolve({
        subscription_id: `sub-${this.requests.length}`,
        session_id: request.session_id,
        high_water_mark_event_id: request.since_event_id,
      } satisfies AppProtocol.SubscribeTimelineResponse as T);
    }
    return Promise.resolve({} as T);
  }

  onEvent(handler: (envelope: AppProtocol.TimelineEventEnvelope) => void): () => void {
    this.eventHandler = handler;
    return () => {
      this.eventHandler = undefined;
    };
  }

  onSubscriptionTerminated(handler: (envelope: AppProtocol.SubscriptionTerminatedEnvelope) => void): () => void {
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

  emitEvent(envelope: AppProtocol.TimelineEventEnvelope): void {
    this.eventHandler?.(envelope);
  }

  emitExit(): void {
    this.exitHandler?.({ code: 1, signal: null });
  }

  emitSubscriptionTerminated(envelope: AppProtocol.SubscriptionTerminatedEnvelope): void {
    this.subscriptionTerminatedHandler?.(envelope);
  }
}

describe("DesktopAppServerController", () => {
  it("restarts app-server and replays subscriptions from the last seen event", async () => {
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
    await controller.invoke("event.subscribe", {
      session_id: "session-1",
      turn_id: null,
      since_event_id: null,
      replay_limit: 50,
      close_when_idle: true,
      close_on_terminal: true,
    } satisfies AppProtocol.SubscribeTimelineRequest);

    bridges[0]?.emitEvent(createEventEnvelope("event-7", "session-1", "turn_started"));
    bridges[0]?.emitExit();

    await vi.advanceTimersByTimeAsync(10);

    expect(bridges).toHaveLength(2);
    expect(bridges[1]?.connected).toBe(true);
    expect(bridges[1]?.requests).toEqual([
      {
        method: "event.subscribe",
        params: {
          session_id: "session-1",
          turn_id: null,
          since_event_id: "event-7",
          replay_limit: 50,
          close_when_idle: true,
          close_on_terminal: true,
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
    const response = await controller.invoke<AppProtocol.SubscribeTimelineResponse>("event.subscribe", {
      session_id: "session-1",
      turn_id: null,
      since_event_id: null,
      replay_limit: 50,
      close_when_idle: null,
      close_on_terminal: null,
    } satisfies AppProtocol.SubscribeTimelineRequest);
    await controller.invoke("event.unsubscribe", {
      subscription_id: response.subscription_id,
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
    const rendererHandlers = new Set<(envelope: AppProtocol.TimelineEventEnvelope) => void>();
    const rendererEvents: AppProtocol.TimelineEventEnvelope[] = [];
    const controller = new DesktopAppServerController({
      restartDelayMs: 10,
      bridgeFactory: () => {
        const bridge = new FakeBridge();
        bridges.push(bridge);
        return bridge;
      },
      onEvent: (envelope) => {
        rendererEvents.push(envelope);
        for (const handler of rendererHandlers) {
          handler(envelope);
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
    const sdkEvents: AppProtocol.TimelineEventEnvelope[] = [];
    const unsubscribe = rendererClient.onEvent((envelope) => sdkEvents.push(envelope));

    try {
      await controller.connect();
      await rendererClient.subscribeEvents({
        session_id: "session-1",
        turn_id: null,
        since_event_id: null,
        replay_limit: 50,
        close_when_idle: null,
        close_on_terminal: null,
      });

      bridges[0]?.emitEvent(createEventEnvelope("event-7", "session-1", "turn_started"));
      bridges[0]?.emitExit();

      await vi.advanceTimersByTimeAsync(10);
      bridges[1]?.emitEvent(
        createEventEnvelope("event-8", "session-1", "turn_completed"),
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
          method: "event.subscribe",
          params: {
            session_id: "session-1",
            turn_id: null,
            since_event_id: "event-7",
            replay_limit: 50,
            close_when_idle: null,
            close_on_terminal: null,
          },
        },
      ]);
      expect(sdkEvents.map((event) => event.event_id)).toEqual(["event-7", "event-8"]);
      expect(rendererEvents.map((event) => event.event_id)).toEqual(["event-7", "event-8"]);
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

function createEventEnvelope(
  eventId: string,
  sessionId: string,
  eventType: "turn_started" | "turn_completed",
): AppProtocol.TimelineEventEnvelope {
  return {
    event_seq: 1,
    event_id: eventId,
    created_at_ms: 1,
    turn_id: "turn-1",
    message_id: "message-1",
    session_id: sessionId,
    event: {
      event_seq: 1,
      event_id: eventId,
      created_at_ms: 1,
      turn_id: "turn-1",
      event_type: eventType,
      message_id: "message-1",
      session_id: sessionId,
      sequence: 0,
      payload:
        eventType === "turn_started"
          ? {
              type: "turn_started",
              request_id: "request-1",
              turn_id: "turn-1",
              started_at_ms: 1,
            }
          : {
              type: "turn_completed",
              request_id: "request-1",
              turn_id: "turn-1",
              completed_at_ms: 2,
              stop_reason: "end_turn",
              usage: {
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cache_creation_input_tokens: null,
                cache_read_input_tokens: null,
              },
              metrics: null,
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

function eventIds(events: AppProtocol.TimelineEventEnvelope[]): string[] {
  return events.map((event) => event.event_id);
}

function writeDesktopSmokeArtifact(artifact: DesktopSmokeArtifact): void {
  const outputDir = path.resolve(process.cwd(), "../../test-results/desktop");
  mkdirSync(outputDir, { recursive: true });
  writeFileSync(
    path.join(outputDir, "app-server-crash-recovery-smoke.json"),
    `${JSON.stringify(artifact, null, 2)}\n`,
  );
}
