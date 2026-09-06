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

  onEvent(handler: (event: AppProtocol.TimelineEvent) => void): () => void {
    return this.inner.onEvent(handler);
  }

  onSubscriptionTerminated(handler: (notification: AppProtocol.SubscriptionTerminatedNotification) => void): () => void {
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
    const rendererHandlers = new Set<(event: AppProtocol.TimelineEvent) => void>();
    const rendererEvents: AppProtocol.TimelineEvent[] = [];
    const stderr: string[] = [];

    controller = new DesktopAppServerController({
      command: process.env.TALOS_APP_SERVER_BIN ?? "app-server",
      cwd: process.env.TALOS_APP_SERVER_CWD,
      clientName: "talos-desktop-crash-recovery-smoke",
      clientVersion: "0.1.5",
      restartDelayMs: 10,
      bridgeFactory: (options: AppServerClientOptions) => {
        const bridge = new RecordingBridge(createDesktopMainAppServerBridge(options));
        bridge.onStderr((chunk) => stderr.push(chunk));
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
      invoke: async <T>(method: AppProtocol.ClientMethod, params: unknown) => {
        if (!controller) throw new Error("controller is closed.");
        return { status: "success", data: await controller.invoke<T>(method, params) };
      },
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
      const sessionId = crypto.randomUUID();

      await rendererClient.subscribeEvents({
        sessionId,
        turnId: null,
        sinceEventId: null,
        replayLimit: 50,
        closeWhenIdle: null,
        closeOnTerminal: null,
      });
      await rendererClient.startTurn({
        sessionId,
        input: { content: [{ type: "text", text: "Answer exactly: desktop-crash-recovery-before" }] },
        modelSelection: null,
        userPermissionMode: null,
        projectId: null,
        currentDirectory: null,
        clientRequestId: "desktop-crash-recovery-before",
      });
      await waitForEvent(
        sdkEvents,
        (event) => event.event.type === "agent" && event.event.data.payload.type === "turnCompleted",
        120_000,
      );
      const lastEventId = requireLastEventId(sdkEvents);

      expect(bridges[0]?.inner.client.transport.terminate("SIGKILL")).toBe(true);
      await waitForLifecycle(lifecycle, ["restarting", "starting", "ready"], 30_000);

      const replaySubscribe = bridges[1]?.requests.find(
        (request) => request.method === "event/subscribe",
      );
      expect(replaySubscribe?.params).toMatchObject({
        sessionId,
        sinceEventId: lastEventId,
        replayLimit: 50,
      });

      await rendererClient.startTurn({
        sessionId,
        input: { content: [{ type: "text", text: "Answer exactly: desktop-crash-recovery-after" }] },
        modelSelection: null,
        userPermissionMode: null,
        projectId: null,
        currentDirectory: null,
        clientRequestId: "desktop-crash-recovery-after",
      });
      await waitForEvent(
        sdkEvents,
        (event) => event.eventId !== lastEventId && event.event.type === "agent" && event.event.data.payload.type === "turnCompleted",
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
  events: AppProtocol.TimelineEvent[],
  predicate: (event: AppProtocol.TimelineEvent) => boolean,
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

function requireLastEventId(events: AppProtocol.TimelineEvent[]): string {
  const event = events.at(-1);
  if (!event) {
    throw new Error("No app-server events were received.");
  }
  return event.eventId;
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

function eventIds(events: AppProtocol.TimelineEvent[]): string[] {
  return events.map((event) => event.eventId);
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
