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
  private eventHandler?: (envelope: AppProtocol.ApplicationEventEnvelope) => void;
  private exitHandler?: (status: { code: number | null; signal: NodeJS.Signals | null }) => void;

  connect(): Promise<AppProtocol.InitializeResponse> {
    this.connected = true;
    return Promise.resolve({
      protocol_version: { major: 0, minor: 3 },
      server_info: { name: "fake", version: "0.1.0" },
    });
  }

  close(): Promise<void> {
    this.closed = true;
    return Promise.resolve();
  }

  invoke<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    this.requests.push({ method, params });
    if (method === "event.subscribe") {
      const request = params as AppProtocol.EventSubscribeParams;
      return Promise.resolve({
        subscription_id: `sub-${this.requests.length}`,
        session_id: request.session_id,
        high_water_mark_event_id: request.since_event_id,
      } satisfies AppProtocol.EventSubscribeResponse as T);
    }
    return Promise.resolve({} as T);
  }

  onEvent(handler: (envelope: AppProtocol.ApplicationEventEnvelope) => void): () => void {
    this.eventHandler = handler;
    return () => {
      this.eventHandler = undefined;
    };
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

  emitEvent(envelope: AppProtocol.ApplicationEventEnvelope): void {
    this.eventHandler?.(envelope);
  }

  emitExit(): void {
    this.exitHandler?.({ code: 1, signal: null });
  }
}

describe("DesktopAppServerController", () => {
  it("restarts app-server and replays subscriptions from the last seen event", async () => {
    vi.useFakeTimers();
    const bridges: FakeBridge[] = [];
    const controller = new DesktopAppServerController({
      launchToken: "token",
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
      since_event_id: null,
      replay_limit: 50,
    } satisfies AppProtocol.EventSubscribeParams);

    bridges[0]?.emitEvent({
      subscription_id: "sub-1",
      event_id: "event-7",
      turn_id: null,
      event: {
        event_type: "message_started",
        message_id: "message-1",
        session_id: "session-1",
        sequence: 0n,
        payload: {
          type: "message_started",
          request_id: "request-1",
          model: "test-model",
          usage: {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cache_creation_input_tokens: null,
            cache_read_input_tokens: null,
          },
          iteration: 0n,
          step_id: "step-1",
        },
      },
    });
    bridges[0]?.emitExit();

    await vi.advanceTimersByTimeAsync(10);

    expect(bridges).toHaveLength(2);
    expect(bridges[1]?.connected).toBe(true);
    expect(bridges[1]?.requests).toEqual([
      {
        method: "event.subscribe",
        params: {
          session_id: "session-1",
          since_event_id: "event-7",
          replay_limit: 50,
        },
      },
    ]);
    vi.useRealTimers();
  });

  it("recovers the renderer app-server stream after a child process exit", async () => {
    vi.useFakeTimers();
    const bridges: FakeBridge[] = [];
    const lifecycle: TalosRuntimeState[] = [];
    const rendererHandlers = new Set<(envelope: AppProtocol.ApplicationEventEnvelope) => void>();
    const rendererEvents: AppProtocol.ApplicationEventEnvelope[] = [];
    const controller = new DesktopAppServerController({
      launchToken: "token",
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
    });
    const sdkEvents: AppProtocol.ApplicationEventEnvelope[] = [];
    const unsubscribe = rendererClient.onEvent((envelope) => sdkEvents.push(envelope));

    try {
      await controller.connect();
      await rendererClient.subscribeEvents({
        session_id: "session-1",
        since_event_id: null,
        replay_limit: 50,
      });

      bridges[0]?.emitEvent(createEventEnvelope("sub-1", "event-7", "session-1", "message_started"));
      bridges[0]?.emitExit();

      await vi.advanceTimersByTimeAsync(10);
      bridges[1]?.emitEvent(
        createEventEnvelope("sub-2", "event-8", "session-1", "message_completed"),
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
            since_event_id: "event-7",
            replay_limit: 50,
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
        rendererEventIds: rendererEvents.map((event) => event.event_id),
        sdkEventIds: sdkEvents.map((event) => event.event_id),
      });
      vi.useRealTimers();
    }
  });
});

function createEventEnvelope(
  subscriptionId: string,
  eventId: string,
  sessionId: string,
  eventType: "message_started" | "message_completed",
): AppProtocol.ApplicationEventEnvelope {
  return {
    subscription_id: subscriptionId,
    event_id: eventId,
    turn_id: null,
    event: {
      event_type: eventType,
      message_id: "message-1",
      session_id: sessionId,
      sequence: 0n,
      payload:
        eventType === "message_started"
          ? {
              type: "message_started",
              request_id: "request-1",
              model: "test-model",
              usage: {
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cache_creation_input_tokens: null,
                cache_read_input_tokens: null,
              },
              iteration: 0n,
              step_id: "step-1",
            }
          : {
              type: "message_completed",
              request_id: "request-1",
              stop_reason: "end_turn",
              usage: {
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cache_creation_input_tokens: null,
                cache_read_input_tokens: null,
              },
              total_output_items: 1n,
              iteration: 0n,
              step_id: "step-1",
              task_count: 0,
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

function writeDesktopSmokeArtifact(artifact: DesktopSmokeArtifact): void {
  const outputDir = path.resolve(process.cwd(), "../../test-results/desktop");
  mkdirSync(outputDir, { recursive: true });
  writeFileSync(
    path.join(outputDir, "app-server-crash-recovery-smoke.json"),
    `${JSON.stringify(artifact, null, 2)}\n`,
  );
}
