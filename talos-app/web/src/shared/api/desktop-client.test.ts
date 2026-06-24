import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("createDesktopTalosClient", () => {
  it("forwards request-object params without nesting session and project ids", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge: DesktopRendererBridge = {
      invoke: async function <T>(method: AppProtocol.ClientMethod, params?: unknown): Promise<T> {
        calls.push({ method, params });
        if (method === "session.get") {
          return { session: { id: "session-1" } } as T;
        }
        if (method === "session.history") {
          return {
            history: {
              session_id: "session-1",
              messages: [],
              generated_at_ms: 1,
              last_stable_event_id: null,
            },
          } as T;
        }
        if (method === "session.active_turn") {
          return {
            active_turn: {
              session_id: "session-1",
              status: "completed",
              turn_id: null,
              message_id: null,
              last_event_id: null,
              pending_user_action: null,
              updated_at: 1,
            },
          } as T;
        }
        if (method === "project.read") {
          return { project: { project_id: "project-1" } } as T;
        }
        if (method === "project.delete") {
          return { success: true } as T;
        }
        throw new Error(`unexpected method: ${method} ${JSON.stringify(params)}`);
      },
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = createDesktopTalosClient(bridge);

    await client.sessions.get({ session_id: "session-1" });
    await expect(client.sessions.history({ session_id: "session-1" })).resolves.toMatchObject({
      history: {
        session_id: "session-1",
      },
    });
    await expect(client.sessions.activeTurn({ session_id: "session-1" })).resolves.toMatchObject({
      active_turn: {
        session_id: "session-1",
      },
    });
    await client.projects.get({ project_id: "project-1" });
    await client.projects.delete({ project_id: "project-1" });

    expect(calls).toEqual([
      { method: "session.get", params: { session_id: "session-1" } },
      { method: "session.history", params: { session_id: "session-1" } },
      { method: "session.active_turn", params: { session_id: "session-1" } },
      { method: "project.read", params: { project_id: "project-1" } },
      { method: "project.delete", params: { project_id: "project-1" } },
    ]);
  });

  it("keeps desktop fs, memory, and workspace document response envelopes aligned with HTTP client", async () => {
    const bridge: DesktopRendererBridge = {
      invoke: async function <T>(method: AppProtocol.ClientMethod, params?: unknown): Promise<T> {
        if (method === "fs.directory.read") {
          return {
            directory: {
              path: "/tmp/project",
              parent: "/tmp",
              entries: [],
              readable: true,
              writable: true,
            },
          } as T;
        }
        if (method === "memory.file.read") {
          return {
            file: {
              date: "2026-06-24",
              path: "memory/2026-06-24.md",
              section_count: 1,
              content: "# Memory",
              updated_at: 1776393847462,
              version_token: "memory-token",
            },
          } as T;
        }
        if (method === "memory.file.write") {
          return {
            file: {
              date: "2026-06-24",
              path: "memory/2026-06-24.md",
              section_count: 1,
              content: "# Updated memory",
              updated_at: 1776393847463,
              version_token: "memory-token-2",
            },
          } as T;
        }
        if (method === "workspace.doc.read") {
          return {
            doc: {
              kind: "memory",
              path: "MEMORY.md",
              exists: true,
              content: "# Workspace memory",
              updated_at: 1776393847462,
              version_token: "doc-token",
            },
          } as T;
        }
        if (method === "workspace.doc.write") {
          return {
            doc: {
              kind: "memory",
              path: "MEMORY.md",
              exists: true,
              content: "# Updated workspace memory",
              updated_at: 1776393847463,
              version_token: "doc-token-2",
            },
          } as T;
        }
        throw new Error(`unexpected method: ${method} ${JSON.stringify(params)}`);
      },
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = createDesktopTalosClient(bridge);

    await expect(client.fs.directory.read({ path: "/tmp/project" })).resolves.toMatchObject({
      directory: {
        path: "/tmp/project",
      },
    });
    await expect(client.memory.get("2026-06-24")).resolves.toMatchObject({
      file: {
        content: "# Memory",
      },
    });
    await expect(client.memory.put("2026-06-24", {
      content: "# Updated memory",
      base_version: { mode: "match", version_token: "memory-token" },
    })).resolves.toMatchObject({
      file: {
        version_token: "memory-token-2",
      },
    });
    await expect(client.workspaceDocs.get("memory")).resolves.toMatchObject({
      doc: {
        content: "# Workspace memory",
      },
    });
    await expect(client.workspaceDocs.put("memory", {
      content: "# Updated workspace memory",
      base_version: { mode: "match", version_token: "doc-token" },
    })).resolves.toMatchObject({
      doc: {
        version_token: "doc-token-2",
      },
    });
  });

  it("uses the desktop source type when starting desktop turns", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge: DesktopRendererBridge = {
      invoke: async function <T>(method: AppProtocol.ClientMethod, params?: unknown): Promise<T> {
        calls.push({ method, params });
        if (method === "turn.start") {
          return {
            request_id: "req-1",
            message_id: "message-1",
            session_id: "session-1",
            turn_id: "turn-1",
            status: "accepted",
          } as T;
        }
        throw new Error(`unexpected method: ${method}`);
      },
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = createDesktopTalosClient(bridge);

    await client.turns.start({
      session_id: "session-1",
      input: [{ type: "text", text: "hello" }],
      context: {
        source_type: "http",
        provider: "sdk",
        user_id: "anonymous",
        channel_id: "web",
        thread_id: null,
        message_id: "msg-1",
        trace_id: "trace-1",
        project_root: null,
        environment: null,
      },
      model: "kimi-k2.5",
    });

    expect(calls).toEqual([
      {
        method: "turn.start",
        params: expect.objectContaining({
          session_id: "session-1",
          context: expect.objectContaining({
            source_type: "desktop",
            provider: "sdk",
            channel_id: "web",
            message_id: "msg-1",
            trace_id: "trace-1",
          }),
        }),
      },
    ]);
  });

  it("streams desktop turn events by message ack even when event request ids differ", async () => {
    let eventHandler: ((envelope: AppProtocol.TimelineEventEnvelope) => void) | undefined;
    const received: string[] = [];
    let subscribeParams: AppProtocol.SubscribeTimelineRequest | undefined;
    const bridge: DesktopRendererBridge = {
      invoke: async function <T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
        if (method === "event.subscribe") {
          subscribeParams = params as AppProtocol.SubscribeTimelineRequest;
          queueMicrotask(() => {
            eventHandler?.(eventEnvelope("turn_started", "req-provider", "turn-1", "message-1"));
            eventHandler?.(eventEnvelope("turn_completed", "req-provider", "turn-1", "message-1"));
          });
          return { subscription_id: "sub-1" } as T;
        }
        if (method === "turn.start") {
          return {
            request_id: "req-client",
            message_id: "message-1",
            session_id: "session-1",
            turn_id: "turn-1",
            status: "accepted",
          } as T;
        }
        if (method === "event.unsubscribe") {
          return { unsubscribed: true } as T;
        }
        throw new Error(`unexpected method: ${method}`);
      },
      onEvent: (handler) => {
        eventHandler = handler;
        return () => {
          eventHandler = undefined;
        };
      },
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = createDesktopTalosClient(bridge);

    await client.turns.startEvents(
      { session_id: "session-1", input: [{ type: "text", text: "hello" }] },
      (envelope) => received.push(envelope.event.event_type),
    );

    expect(received).toEqual(["turn_started", "turn_completed"]);
    expect(subscribeParams).toMatchObject({
      close_when_idle: true,
      close_on_terminal: true,
    });
  });
});

function eventEnvelope(
  eventType: "turn_started" | "turn_completed",
  requestId: string,
  turnId: string,
  messageId: string,
): AppProtocol.TimelineEventEnvelope {
  return {
    event_seq: 1,
    event_id: `ev-${eventType}`,
    created_at_ms: 1,
    turn_id: turnId,
    message_id: messageId,
    session_id: "session-1",
    event: {
      event_seq: 1,
      event_id: `ev-${eventType}`,
      created_at_ms: 1,
      turn_id: turnId,
      session_id: "session-1",
      message_id: messageId,
      sequence: 1,
      event_type: eventType,
      payload:
        eventType === "turn_started"
          ? {
              type: "turn_started",
              request_id: requestId,
              turn_id: turnId,
              started_at_ms: 1,
            }
          : {
              type: "turn_completed",
              request_id: requestId,
              turn_id: turnId,
              completed_at_ms: 2,
              stop_reason: "end_turn",
              usage: usage(),
              metrics: null,
            },
    },
  };
}

function usage(): AppProtocol.TalosUsage {
  return {
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
    cache_creation_input_tokens: null,
    cache_read_input_tokens: null,
  };
}
