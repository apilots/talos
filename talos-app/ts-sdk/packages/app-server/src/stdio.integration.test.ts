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
      clientVersion: "0.1.2",
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
    expect(models.length).toBeGreaterThan(0);

    const sessionId = crypto.randomUUID();

    const events: AppProtocol.TimelineEventEnvelope[] = [];
    const unsubscribe = client.onEvent((event) => events.push(event));
    try {
      await client.subscribeEvents({
        session_id: sessionId,
        since_event_id: null,
        replay_limit: 50,
        close_when_idle: null,
        close_on_terminal: null,
      });
      await client.startTurn({
        session_id: sessionId,
        input: [{ type: "text", text: "Answer exactly: ts-sdk-app-server-ok" }],
        workspace_id: null,
        context: {
          source_type: "cli",
          provider: "cli",
          channel_id: "ts-sdk-integration",
          thread_id: null,
          message_id: `msg_${Date.now()}`,
          trace_id: `trace_${Date.now()}`,
        },
        model: null,
        project_id: null,
        current_directory: null,
        client_request_id: "ts-sdk-app-server-smoke",
      });
      await waitFor(() => events.some((event) => event.event.event_type === "turn_completed"), 90_000);
    } finally {
      unsubscribe();
    }

    expect(events.some((event) => event.event.event_type === "turn_started")).toBe(true);
    expect(events.some((event) => event.event.event_type === "item_delta")).toBe(true);
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
