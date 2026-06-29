import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";

import type { AppProtocol } from "@talos/sdk-core";
import {
  createDesktopMainAppServerBridge,
  type AppServerClientOptions,
  type DesktopMainAppServerBridge,
} from "@talos/sdk-app-server/desktop-main";
import { createElectronAppServerClient } from "@talos/sdk-app-server/desktop";
import { afterEach, describe, expect, it } from "vitest";

import type { AppServerBridge } from "./app-server-controller";
import { DesktopAppServerController } from "./app-server-controller";
import type { TalosRuntimeState } from "./ipc";

const runIntegration = process.env.TALOS_DESKTOP_APP_SERVER_INTEGRATION === "1";
const describeIntegration = runIntegration ? describe : describe.skip;

class RecordingBridge implements AppServerBridge {
  readonly requests: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];

  constructor(readonly inner: DesktopMainAppServerBridge) {}

  connect(): Promise<AppProtocol.InitializeResponse> {
    return this.inner.connect();
  }

  close(): Promise<void> {
    return this.inner.close();
  }

  invoke<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    this.requests.push({ method, params });
    return this.inner.invoke<T>(method, params);
  }

  onEvent(handler: (envelope: AppProtocol.TimelineEventEnvelope) => void): () => void {
    return this.inner.onEvent(handler);
  }

  onSubscriptionTerminated(handler: (envelope: AppProtocol.SubscriptionTerminatedEnvelope) => void): () => void {
    return this.inner.onSubscriptionTerminated(handler);
  }

  onError(handler: (error: Error) => void): () => void {
    return this.inner.onError(handler);
  }

  onExit(handler: (status: { code: number | null; signal: NodeJS.Signals | null }) => void): () => void {
    return this.inner.onExit(handler);
  }

  onStderr(handler: (chunk: string) => void): () => void {
    return this.inner.onStderr(handler);
  }

  onServerRequest(handler: (request: AppProtocol.JsonRpcRequest) => Promise<unknown> | unknown): () => void {
    return this.inner.onServerRequest(handler);
  }
}

describeIntegration("DesktopAppServerController app-server crash recovery", () => {
  let controller: DesktopAppServerController | undefined;

  afterEach(async () => {
    await controller?.close();
    controller = undefined;
  });

  it("restarts the stdio app-server and replays renderer subscriptions", async () => {
    const bridges: RecordingBridge[] = [];
    const lifecycle: TalosRuntimeState[] = [];
    const rendererHandlers = new Set<(envelope: AppProtocol.TimelineEventEnvelope) => void>();
    const rendererEvents: AppProtocol.TimelineEventEnvelope[] = [];
    const stderr: string[] = [];

    controller = new DesktopAppServerController({
      command: process.env.TALOS_APP_SERVER_BIN ?? "app-server",
      cwd: process.env.TALOS_APP_SERVER_CWD,
      clientName: "talos-desktop-crash-recovery-smoke",
      clientVersion: "0.1.3",
      restartDelayMs: 10,
      bridgeFactory: (options: AppServerClientOptions) => {
        const bridge = new RecordingBridge(createDesktopMainAppServerBridge(options));
        bridge.onStderr((chunk) => stderr.push(chunk));
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
      invoke: (method, params) => controller?.invoke(method, params) ?? Promise.reject(new Error("controller is closed.")),
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
      const sessionId = crypto.randomUUID();

      await rendererClient.subscribeEvents({
        session_id: sessionId,
        turn_id: null,
        since_event_id: null,
        replay_limit: 50,
        close_when_idle: null,
        close_on_terminal: null,
      });
      await rendererClient.startTurn({
        session_id: sessionId,
        input: [{ type: "text", text: "Answer exactly: desktop-crash-recovery-before" }],
        context: {
          source_type: "cli",
          provider: "cli",
          user_id: "desktop-crash-recovery",
          channel_id: "desktop-crash-recovery",
          thread_id: null,
          message_id: `msg_${Date.now()}`,
          trace_id: `trace_${Date.now()}`,
          project_root: null,
          environment: null,
        },
        client_request_id: "desktop-crash-recovery-before",
      });
      await waitForEvent(
        sdkEvents,
        (event) => event.event.event_type === "turn_completed",
        120_000,
      );
      const lastEventId = requireLastEventId(sdkEvents);

      expect(bridges[0]?.inner.client.transport.terminate("SIGKILL")).toBe(true);
      await waitForLifecycle(lifecycle, ["restarting", "starting", "ready"], 30_000);

      const replaySubscribe = bridges[1]?.requests.find(
        (request) => request.method === "event.subscribe",
      );
      expect(replaySubscribe?.params).toMatchObject({
        session_id: sessionId,
        since_event_id: lastEventId,
        replay_limit: 50,
      });

      await rendererClient.startTurn({
        session_id: sessionId,
        input: [{ type: "text", text: "Answer exactly: desktop-crash-recovery-after" }],
        client_request_id: "desktop-crash-recovery-after",
      });
      await waitForEvent(
        sdkEvents,
        (event) => event.event_id !== lastEventId && event.event.event_type === "turn_completed",
        120_000,
      );

      expect(lifecycle.map((state) => state.kind)).toEqual([
        "starting",
        "ready",
        "restarting",
        "starting",
        "ready",
      ]);
      expect(rendererEvents.length).toBeGreaterThan(0);
      expect(sdkEvents.length).toBeGreaterThan(0);
    } catch (error) {
      throw new Error(`${errorMessage(error)}\n${stderr.join("")}`);
    } finally {
      unsubscribe();
      writeDesktopSmokeArtifact({
        lifecycle,
        bridgeRequests: bridges.map((bridge) => bridge.requests),
        rendererEventIds: eventIds(rendererEvents),
        sdkEventIds: eventIds(sdkEvents),
      });
    }
  }, 300_000);
});

async function waitForEvent(
  events: AppProtocol.TimelineEventEnvelope[],
  predicate: (event: AppProtocol.TimelineEventEnvelope) => boolean,
  timeoutMs: number,
): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (events.some(predicate)) {
      return;
    }
    await sleep(100);
  }
  throw new Error("Timed out waiting for app-server event.");
}

async function waitForLifecycle(
  lifecycle: TalosRuntimeState[],
  expectedSuffix: TalosRuntimeState["kind"][],
  timeoutMs: number,
): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (containsOrdered(lifecycle.map((state) => state.kind), expectedSuffix)) {
      return;
    }
    await sleep(100);
  }
  throw new Error(`Timed out waiting for lifecycle: ${expectedSuffix.join(", ")}`);
}

function containsOrdered<T>(values: T[], expected: T[]): boolean {
  let expectedIndex = 0;
  for (const value of values) {
    if (value === expected[expectedIndex]) {
      expectedIndex += 1;
    }
    if (expectedIndex === expected.length) {
      return true;
    }
  }
  return false;
}

function requireLastEventId(events: AppProtocol.TimelineEventEnvelope[]): string {
  const event = events.at(-1);
  if (!event) {
    throw new Error("No app-server events were received.");
  }
  return event.event_id;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
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
    path.join(outputDir, "real-app-server-crash-recovery-smoke.json"),
    `${JSON.stringify(artifact, null, 2)}\n`,
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
