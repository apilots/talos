import { afterEach, describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";

import { StdioAppServerClient } from "./stdio";

const runIntegration = process.env.TALOS_TS_APP_SERVER_INTEGRATION === "1";
const describeIntegration = runIntegration ? describe : describe.skip;

describeIntegration("StdioAppServerClient integration", () => {
  const clients: StdioAppServerClient[] = [];

  afterEach(async () => {
    await Promise.all(clients.map((client) => client.close()));
    clients.length = 0;
  });

  it("creates a session, starts a turn, and receives app-server events", async () => {
    const client = new StdioAppServerClient({
      command: process.env.TALOS_APP_SERVER_BIN ?? "app-server",
      cwd: process.env.TALOS_APP_SERVER_CWD,
      clientName: "talos-ts-sdk-integration",
      clientVersion: "0.1.5",
    });
    const stderr: string[] = [];
    client.transport.onStderr((chunk) => stderr.push(chunk));
    clients.push(client);

    try {
      await client.connect();
    } catch (error) {
      throw new Error(`${errorMessage(error)}\n${stderr.join("")}`);
    }
    const models = await client.listModels();
    expect(models.models.length).toBeGreaterThan(0);

    const sessionId = crypto.randomUUID();

    const events: AppProtocol.TimelineEvent[] = [];
    const unsubscribe = client.onEvent((event) => events.push(event));
    try {
      await client.subscribeEvents({
        sessionId,
        turnId: null,
        sinceEventId: null,
        replayLimit: 50,
        closeWhenIdle: null,
        closeOnTerminal: null,
      });
      await client.startTurn({
        sessionId,
        input: { content: [{ type: "text", text: "Answer exactly: ts-sdk-app-server-ok" }] },
        modelSelection: null,
        userPermissionMode: null,
        projectId: null,
        currentDirectory: null,
        clientRequestId: "ts-sdk-app-server-smoke",
      });
      await waitFor(
        () => events.some((event) => event.event.type === "agent" && event.event.data.payload.type === "turnCompleted"),
        90_000,
      );
    } finally {
      unsubscribe();
    }

    expect(events.some((event) => event.event.type === "agent" && event.event.data.payload.type === "turnStarted")).toBe(true);
    expect(events.some((event) => event.event.type === "agent" && event.event.data.payload.type === "messageBlockDelta")).toBe(true);
  }, 120_000);
});

async function waitFor(predicate: () => boolean, timeoutMs: number): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => {
      setTimeout(resolve, 100);
    });
  }
  throw new Error("Timed out waiting for app-server event.");
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
