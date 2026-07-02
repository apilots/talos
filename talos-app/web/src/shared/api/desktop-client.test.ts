import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("createDesktopTalosClient", () => {
  it("forwards request-object params without nesting session and project ids", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
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
    });
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
    const bridge = bridgeWithResponses({
      "fs.directory.read": {
            directory: {
              path: "/tmp/project",
              parent: "/tmp",
              entries: [],
              readable: true,
              writable: true,
            },
      },
      "memory.file.read": {
            file: {
              date: "2026-06-24",
              section_count: 1,
              content: "# Memory",
              updated_at: 1776393847462,
              version_token: "memory-token",
            },
      },
      "memory.file.write": {
            file: {
              date: "2026-06-24",
              section_count: 1,
              content: "# Updated memory",
              updated_at: 1776393847463,
              version_token: "memory-token-2",
            },
      },
      "memory.search": {
            results: [
              {
                date: "2026-06-24",
                snippet: "Updated memory",
                start_line: 1,
                end_line: 1,
              },
            ],
      },
      "workspace.doc.read": {
            doc: {
              kind: "memory",
              path: "MEMORY.md",
              exists: true,
              content: "# Workspace memory",
              updated_at: 1776393847462,
              version_token: "doc-token",
            },
      },
      "workspace.doc.write": {
            doc: {
              kind: "memory",
              path: "MEMORY.md",
              exists: true,
              content: "# Updated workspace memory",
              updated_at: 1776393847463,
              version_token: "doc-token-2",
            },
      },
    });
    const client = createDesktopTalosClient(bridge);

    await expect(client.fs.directory.read({ path: "/tmp/project" })).resolves.toMatchObject({ directory: { path: "/tmp/project" } });
    await expect(client.memory.get({ date: "2026-06-24" })).resolves.toMatchObject({ file: { content: "# Memory" } });
    await expect(client.memory.put({
      date: "2026-06-24",
      content: "# Updated memory",
      expected_version: "memory-token",
    })).resolves.toMatchObject({ file: { version_token: "memory-token-2" } });
    await expect(client.memory.search({
      query: "Updated",
      limit: 5,
      date_from: "2026-06-01",
      date_to: "2026-06-30",
    })).resolves.toMatchObject({ results: [{ date: "2026-06-24" }] });
    await expect(client.workspaceDocs.get("memory")).resolves.toMatchObject({ doc: { content: "# Workspace memory" } });
    await expect(client.workspaceDocs.put("memory", {
      content: "# Updated workspace memory",
      base_version: { mode: "match", version_token: "doc-token" },
    })).resolves.toMatchObject({ doc: { version_token: "doc-token-2" } });
  });

  it("passes memory search query and date filters to the desktop bridge", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
        calls.push({ method, params });
        if (method === "memory.search") {
          return { results: [] } as T;
        }
        throw new Error(`unexpected method: ${method}`);
    });
    const client = createDesktopTalosClient(bridge);

    await client.memory.search({
      query: "memory",
      limit: 5,
      date_from: "2026-06-01",
      date_to: "2026-06-30",
    });

    expect(calls).toEqual([
      {
        method: "memory.search",
        params: {
          query: "memory",
          limit: 5,
          date_from: "2026-06-01",
          date_to: "2026-06-30",
        },
      },
    ]);
  });

  it("passes memory file request objects to the desktop bridge", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
        calls.push({ method, params });
        if (method === "memory.file.read") {
          return { file: { date: "2026-06-24", content: "# Memory" } } as T;
        }
        if (method === "memory.file.write") {
          return { file: { date: "2026-06-24", content: "# Updated memory" } } as T;
        }
        throw new Error(`unexpected method: ${method}`);
    });
    const client = createDesktopTalosClient(bridge);

    await client.memory.get({ date: "2026-06-24" });
    await client.memory.put({
      date: "2026-06-24",
      content: "# Updated memory",
      expected_version: "memory-token",
    });
    await client.memory.put({
      date: "2026-06-25",
      content: "# New memory",
    });

    expect(calls).toEqual([
      { method: "memory.file.read", params: { date: "2026-06-24" } },
      {
        method: "memory.file.write",
        params: {
          date: "2026-06-24",
          content: "# Updated memory",
          expected_version: "memory-token",
        },
      },
      {
        method: "memory.file.write",
        params: {
          date: "2026-06-25",
          content: "# New memory",
        },
      },
    ]);
  });

  it("uses the desktop source type when starting desktop turns", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
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
    });
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

});

function bridgeWithInvoke(invoke: DesktopRendererBridge["invoke"]): DesktopRendererBridge {
  return {
    invoke,
    onEvent: () => () => undefined,
    onSubscriptionTerminated: () => () => undefined,
  };
}

function bridgeWithResponses(
  responses: Partial<Record<AppProtocol.ClientMethod, unknown>>,
): DesktopRendererBridge {
  return bridgeWithInvoke(async function <T>(method: AppProtocol.ClientMethod, params?: unknown) {
    if (Object.prototype.hasOwnProperty.call(responses, method)) {
      return responses[method] as T;
    }
    throw new Error(`unexpected method: ${method} ${JSON.stringify(params)}`);
  });
}
