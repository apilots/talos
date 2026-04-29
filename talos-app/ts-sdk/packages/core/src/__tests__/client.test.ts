import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";
import type { HttpAdapter, StreamAdapter } from "../adapters";

describe("TalosClient", () => {
  it("uses absolute urls for http requests", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080/");

    await client.models.list();

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/models");
  });

  it("uses ack plus events for streaming messages", async () => {
    const http: HttpAdapter = {
      request: vi.fn().mockResolvedValue({
        request_id: "req-1",
        message_id: "msg-1",
        session_id: "session-1",
        turn_id: "turn-1",
        status: "accepted",
      }),
    };
    const stream = vi.fn().mockImplementation(async (_url, _options, onMessage) => {
      onMessage(JSON.stringify({
        event_seq: 1,
        event_id: "ev_00000000000000000001",
        session_id: "session-1",
        turn_id: "turn-1",
        message_id: "msg-1",
        sequence: 1,
        event_type: "message_completed",
        payload: { type: "message_completed", request_id: "req-1" },
        created_at: 1,
      }));
    });
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.messages.sendEvents(
      "session-1",
      {
        input: [{ type: "text", text: "hello" }],
      },
      () => undefined,
    );

    expect(http.request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/messages",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          input: [{ type: "text", text: "hello" }],
        }),
      }),
    );
    expect(stream).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/events",
      expect.objectContaining({
        method: "GET",
      }),
      expect.any(Function),
    );
  });

  it("filters session events by message id in the SDK helper", async () => {
    const http: HttpAdapter = {
      request: vi.fn(),
    };
    const onEvent = vi.fn();
    const stream = vi.fn().mockImplementation(async (_url, options, onMessage) => {
      onMessage(JSON.stringify({
        event_seq: 1,
        event_id: "ev_00000000000000000001",
        session_id: "session-1",
        turn_id: "turn-1",
        message_id: "other-message",
        sequence: 1,
        event_type: "message_output_delta",
        payload: { type: "message_output_delta", request_id: "req-1", delta: "skip" },
        created_at: 1,
      }));
      onMessage(JSON.stringify({
        event_seq: 2,
        event_id: "ev_00000000000000000002",
        session_id: "session-1",
        turn_id: "turn-1",
        message_id: "msg-1",
        sequence: 2,
        event_type: "message_completed",
        payload: { type: "message_completed", request_id: "req-1" },
        created_at: 2,
      }));
      expect(options.signal.aborted).toBe(true);
    });
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.messages.eventsForMessage("session-1", "msg-1", onEvent);

    expect(onEvent).toHaveBeenCalledTimes(1);
    expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({
      event_type: "message_completed",
      message_id: "msg-1",
    }));
  });

  it("uses message attempts for business retry execution", async () => {
    const request = vi.fn().mockResolvedValue({
      request_id: "req-1",
      message_id: "msg-2",
      session_id: "session-1",
      turn_id: "turn-2",
      status: "accepted",
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.messages.retry("session-1", {
      client_message_id: "client-1",
      retry_of_message_id: "msg-1",
      input: [{ type: "text", text: "retry" }],
    });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/message-attempts",
      expect.objectContaining({
        method: "POST",
        headers: { "idempotency-key": "retry_client-1_msg-1" },
        body: JSON.stringify({
          client_message_id: "client-1",
          retry_of_message_id: "msg-1",
          input: [{ type: "text", text: "retry" }],
        }),
      }),
    );
  });

  it("uses the dedicated close route for session shutdown", async () => {
    const request = vi.fn().mockResolvedValue({ success: true });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.sessions.close("session-1");

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/sessions/session-1/close", {
      method: "POST",
    });
  });

  it("reads the active turn recovery snapshot", async () => {
    const request = vi.fn().mockResolvedValue({
      session_id: "session-1",
      turn_id: "turn-1",
      message_id: "msg-1",
      status: "waiting_user_action",
      last_event_id: "ev_00000000000000000007",
      pending_user_action: {
        type: "dynamic_tool_result",
        call_id: "call-1",
        tool_name: "ExternalTool",
      },
      updated_at: 123,
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    const activeTurn = await client.sessions.activeTurn("session-1");

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/active-turn",
    );
    expect(activeTurn.pending_user_action).toMatchObject({
      type: "dynamic_tool_result",
      call_id: "call-1",
    });
  });

  it("posts project creation to the project collection route", async () => {
    const request = vi.fn().mockResolvedValue({
      project_id: "project-1",
      name: "repo",
      directory: "/repo",
      created_at: 1,
      updated_at: 1,
      session_count: 0,
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.projects.create({ name: "repo", directory: "/repo" });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/projects", {
      method: "POST",
      body: JSON.stringify({ name: "repo", directory: "/repo" }),
    });
  });

  it("sends project deletion to the project resource route", async () => {
    const request = vi.fn().mockResolvedValue(undefined);
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.projects.delete("project-1");

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/projects/project-1", {
      method: "DELETE",
    });
  });

  it("sends admin session deletion to the admin session resource route", async () => {
    const request = vi.fn().mockResolvedValue(undefined);
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.admin.sessions.delete("session-1");

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/admin/sessions/session-1", {
      method: "DELETE",
    });
  });

  it("serializes admin session pagination parameters and returns pagination metadata", async () => {
    const request = vi.fn().mockResolvedValue({
      data: [],
      pagination: {
        total: 42,
        limit: 20,
        offset: 20,
      },
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    const response = await client.admin.sessions.list({ limit: 20, offset: 20 });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/admin/sessions?limit=20&offset=20");
    expect(response.pagination.total).toBe(42);
  });

  it("serializes session query parameters on the sessions list url", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.sessions.list({ project_id: "project-1" });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/sessions?project_id=project-1");
  });

  it("serializes memory search query parameters on the request url", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.memory.search({
      q: "hello",
      limit: 5,
      date_from: "2026-04-01",
      date_to: "2026-04-30",
    });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/memory/search?q=hello&limit=5&date_from=2026-04-01&date_to=2026-04-30",
    );
  });

  it("forwards abort signals to memory search requests", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");
    const controller = new AbortController();

    await client.memory.search({ q: "hello" }, { signal: controller.signal });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/memory/search?q=hello",
      { signal: controller.signal },
    );
  });

  it("writes workspace docs through the workspace docs route", async () => {
    const request = vi.fn().mockResolvedValue({
      kind: "memory",
      path: "MEMORY.md",
      exists: true,
      content: "body",
      updated_at: 1,
      version_token: "v1",
    });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.workspaceDocs.put("memory", {
      content: "body",
      base_version: { mode: "must_not_exist" },
    });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/workspace/docs/memory", {
      method: "PUT",
      body: JSON.stringify({
        content: "body",
        base_version: { mode: "must_not_exist" },
      }),
    });
  });
});
