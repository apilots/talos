import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import { describe, expect, it, vi } from "vitest";
import { AppProtocol } from "@talos/sdk-core";
import { createElectronAppServerClient } from "@talos/sdk-app-server/desktop";

import { FakeBridge } from "./app-server-controller.test-support";
import { DesktopAppServerController } from "./app-server-controller";
import type { TalosRuntimeState } from "./ipc";

describe("DesktopAppServerController", () => {
  it("forwards real-time active and queue notices and clears an old Worker failure after recovery", async () => {
    const bridge = new FakeBridge();
    const onActiveTurnChanged = vi.fn();
    const onMessageQueueChanged = vi.fn();
    const onRuntimeState = vi.fn();
    const controller = new DesktopAppServerController({ bridgeFactory: () => bridge, onActiveTurnChanged, onMessageQueueChanged, onRuntimeState });
    await controller.connect();
    const active = { sessionId: "shared", state: { activeTurn: null, latestEventId: null } };
    bridge.emitNotification({ jsonrpc: "2.0", method: "session/activeTurnChanged", params: active });
    bridge.emitNotification({ jsonrpc: "2.0", method: "messageQueue/changed", params: { sessionId: "shared", revision: 2 } });
    bridge.emitNotification({ jsonrpc: "2.0", method: "desktop/worker/failed", params: { code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Computer unavailable", localization: null } });
    bridge.emitNotification({ jsonrpc: "2.0", method: "desktop/worker/ready", params: null });
    expect(onActiveTurnChanged).toHaveBeenCalledWith(active);
    expect(onMessageQueueChanged).toHaveBeenCalledWith({ sessionId: "shared", revision: 2 });
    expect(onRuntimeState.mock.calls.at(-1)).toEqual([{ kind: "ready", message: null }]);
    await controller.close();
  });

  it("terminates subscriptions when background reconnect fails without a waiting request", async () => {
    vi.useFakeTimers();
    const bridge = new FakeBridge();
    const replacement = new FakeBridge();
    vi.spyOn(replacement, "connect").mockRejectedValue(new Error("Reconnect failed."));
    const bridgeFactory = vi.fn().mockReturnValueOnce(bridge).mockReturnValue(replacement);
    const terminated = vi.fn();
    const report = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const controller = new DesktopAppServerController({ bridgeFactory, onSubscriptionTerminated: terminated });
    try {
      await controller.connect();
      await controller.invoke("event/subscribe", { sessionId: "session-1", turnId: null, sinceEventId: null, replayLimit: null });
      bridge.emitExit();
      await vi.advanceTimersByTimeAsync(300);
      expect(terminated).toHaveBeenCalledOnce();
      expect(report).toHaveBeenCalledOnce();
    } finally {
      await controller.close();
      report.mockRestore();
      vi.useRealTimers();
    }
  });

  it("terminates active subscriptions when the transport rejects malformed events", async () => {
    const bridge = new FakeBridge();
    const terminated = vi.fn();
    const report = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const controller = new DesktopAppServerController({ bridgeFactory: () => bridge, onSubscriptionTerminated: terminated });
    try {
      await controller.connect();
      await controller.invoke("event/subscribe", { sessionId: "session-1", turnId: null, sinceEventId: null, replayLimit: null, closeWhenIdle: false, closeOnTerminal: false });
      bridge.emitError(new Error("SECRET_INVALID_FRAME"));
      expect(terminated.mock.calls).toEqual([[{ subscriptionId: "sub-1", sessionId: "session-1", reason: "interrupted", message: "The app-server event stream failed. Please reconnect." }]]);
      expect(report).toHaveBeenCalledOnce();
    } finally {
      await controller.close();
      report.mockRestore();
    }
  });

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
      invoke: async <T>(method: AppProtocol.ClientMethod, params: unknown) => ({
        status: "success", data: await controller.invoke<T>(method, params),
      }),
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
    event: { type: "agent", data: {
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
    } },
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
