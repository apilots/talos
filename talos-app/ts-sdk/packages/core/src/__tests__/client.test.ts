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

  it("forces stream=true for streaming messages", async () => {
    const http: HttpAdapter = {
      request: vi.fn(),
    };
    const stream = vi.fn().mockResolvedValue(undefined);
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.messages.sendStream(
      "session-1",
      {
        input: [{ type: "text", text: "hello" }],
      },
      () => undefined,
    );

    expect(stream).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/messages",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          input: [{ type: "text", text: "hello" }],
          stream: true,
        }),
      }),
      expect.any(Function),
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
