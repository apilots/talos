import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("createDesktopTalosClient", () => {
  it("submits typed Turn input through the app-server transport", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
      calls.push({ method, params });
      return {
        requestId: "request-1",
        identity: { sessionId: "session-1", turnId: "turn-1" },
        messageId: "message-1",
      } as T;
    });
    const client = createDesktopTalosClient(bridge);
    const request: AppProtocol.SubmitTurnInputRequest = {
      identity: { sessionId: "session-1", turnId: "turn-1" },
      input: {
        type: "userActionResponse",
        actionId: "action-1",
        response: { type: "question", response: { type: "selectedOption", optionId: "yes" } },
      },
      clientRequestId: null,
    };

    await client.turns.input(request);

    expect(calls).toEqual([{ method: "turn/input", params: request }]);
  });

  it("forwards request-object params without nesting session and project ids", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
        calls.push({ method, params });
        if (method === "session/get") {
          return { session: { id: "session-1" } } as T;
        }
        if (method === "session/history") {
          return {
            data: [],
            nextCursor: null,
          } as T;
        }
        if (method === "session/active_turn") {
          return { activeTurn: null } as T;
        }
        if (method === "project/delete") {
          return { success: true } as T;
        }
        throw new Error(`unexpected method: ${method} ${JSON.stringify(params)}`);
    });
    const client = createDesktopTalosClient(bridge);

    await client.sessions.get({ sessionId: "session-1" });
    await expect(
      client.sessions.history({ sessionId: "session-1", cursor: null, limit: null }),
    ).resolves.toEqual({ data: [], nextCursor: null });
    await expect(client.sessions.activeTurn({ sessionId: "session-1" })).resolves.toEqual({
      activeTurn: null,
    });
    await client.projects.delete({ project_id: "project-1" });

    expect(calls).toEqual([
      { method: "session/get", params: { sessionId: "session-1" } },
      {
        method: "session/history",
        params: { sessionId: "session-1", cursor: null, limit: null },
      },
      { method: "session/active_turn", params: { sessionId: "session-1" } },
      { method: "project/delete", params: { project_id: "project-1" } },
    ]);
  });

  it("keeps desktop fs, memory, and workspace document response envelopes aligned with HTTP client", async () => {
    const bridge = bridgeWithResponses({
      "workspace/info": { workspaceId: "team", rootPrefix: "/srv/team", userWorkspaceDir: "/srv/team/alice" },
      "fs/directory/read": {
            directory: {
              path: "/tmp/project",
              parent: "/tmp",
              entries: [],
              readable: true,
              writable: true,
            },
      },
      "memory/file/read": {
            file: {
              date: "2026-06-24",
              section_count: 1,
              content: "# Memory",
              updated_at: 1776393847462,
              version_token: "memory-token",
            },
      },
      "memory/file/write": {
            file: {
              date: "2026-06-24",
              section_count: 1,
              content: "# Updated memory",
              updated_at: 1776393847463,
              version_token: "memory-token-2",
            },
      },
      "workspace/doc/read": {
            doc: {
              kind: "memory",
              path: "MEMORY.md",
              exists: true,
              content: "# Workspace memory",
              updated_at: 1776393847462,
              version_token: "doc-token",
            },
      },
      "workspace/doc/write": {
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
    await expect(client.workspaceDocs.info()).resolves.toEqual({ workspaceId: "team", rootPrefix: "/srv/team", userWorkspaceDir: "/srv/team/alice" });
    await expect(client.workspaceDocs.get("memory")).resolves.toMatchObject({ doc: { content: "# Workspace memory" } });
    await expect(client.workspaceDocs.put("memory", {
      content: "# Updated workspace memory",
      base_version: { mode: "match", version_token: "doc-token" },
    })).resolves.toMatchObject({ doc: { version_token: "doc-token-2" } });
  });

  it("passes memory search filters to the desktop bridge", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
        calls.push({ method, params });
        if (method === "memory/search") {
          return { data: [] } as T;
        }
        throw new Error(`unexpected method: ${method}`);
    });
    const client = createDesktopTalosClient(bridge);

    await client.memory.search({
      query: "memory",
      limit: 5,
      sinceAt: 1,
      untilAt: 2,
    });

    expect(calls).toEqual([
      {
        method: "memory/search",
        params: {
          query: "memory",
          limit: 5,
          sinceAt: 1,
          untilAt: 2,
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
        if (method === "memory/file/read") {
          return { file: { date: "2026-06-24", content: "# Memory" } } as T;
        }
        if (method === "memory/file/write") {
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
      { method: "memory/file/read", params: { date: "2026-06-24" } },
      {
        method: "memory/file/write",
        params: {
          date: "2026-06-24",
          content: "# Updated memory",
          expected_version: "memory-token",
        },
      },
      {
        method: "memory/file/write",
        params: {
          date: "2026-06-25",
          content: "# New memory",
        },
      },
    ]);
  });

  it("forwards the canonical start-turn request without source metadata", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
        calls.push({ method, params });
        if (method === "turn/start") {
          return {
            requestId: "req-1",
            messageId: "message-1",
            sessionId: "session-1",
            turnId: "turn-1",
            status: "accepted",
          } as T;
        }
        throw new Error(`unexpected method: ${method}`);
    });
    const client = createDesktopTalosClient(bridge);

    await client.turns.start({
      sessionId: "session-1",
      input: { content: [{ type: "text", text: "hello" }] },
      modelSelection: {
        modelRef: { provider: "bailian", id: "kimi-k2.5" },
        reasoningEffort: null,
      },
      userPermissionMode: null,
      projectId: null,
      currentDirectory: null,
      clientRequestId: null,
    });

    expect(calls).toEqual([
      {
        method: "turn/start",
        params: expect.objectContaining({
          sessionId: "session-1",
          modelSelection: {
            modelRef: { provider: "bailian", id: "kimi-k2.5" },
            reasoningEffort: null,
          },
        }),
      },
    ]);
  });

});

function bridgeWithInvoke(invoke: <T>(method: AppProtocol.ClientMethod, params?: unknown) => Promise<T>): DesktopRendererBridge {
  return {
    invoke: async <T>(method: AppProtocol.ClientMethod, params: unknown) => ({ status: "success", data: await invoke<T>(method, params) }),
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
