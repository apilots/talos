import { describe, expect, it, vi } from "vitest";
import { TalosClient } from "../client";
import { AppServerClient } from "../app-server";
import type { HttpAdapter, StreamAdapter } from "../adapters";
import { RecordingTransport } from "./app-server.fixture";

describe("Computer routing through both client transports", () => {
  it("uses normalized absolute URLs for workspace info and model requests", async () => {
    const info = { workspaceId: "team", rootPrefix: "/srv/team", userWorkspaceDir: "/srv/team/alice" };
    const request = vi.fn().mockResolvedValue(info);
    const client = new TalosClient({ request }, noopStreamAdapter(), "http://localhost:8080/");
    await expect(client.workspaceDocs.info()).resolves.toEqual(info);
    await client.models.list();
    expect(request).toHaveBeenNthCalledWith(1, "http://localhost:8080/v1/workspace/info");
    expect(request).toHaveBeenNthCalledWith(2, "http://localhost:8080/v1/models");
  });

  it("routes workspace, model and project queries to the selected Computer", async () => {
    const request = vi.fn().mockResolvedValue({});
    const client = new TalosClient({ request }, noopStreamAdapter(), "http://localhost:8080/");
    await client.workspaceDocs.info({ nodeId: "worker-1" });
    await client.models.list({ nodeId: "worker-1" });
    await client.projects.list({ nodeId: "worker-1" });
    expect(request.mock.calls.map(([url]) => url)).toEqual([
      "http://localhost:8080/v1/workspace/info?nodeId=worker-1",
      "http://localhost:8080/v1/models?nodeId=worker-1",
      "http://localhost:8080/v1/projects?nodeId=worker-1",
    ]);
  });

  it("keeps the selected Computer across directory and document operations", async () => {
    const request = vi.fn().mockResolvedValue({});
    const client = new TalosClient({ request }, noopStreamAdapter(), "http://localhost:8080");
    const target = { nodeId: "worker-b" };
    await client.config.get("llm", target);
    await client.config.reload(target);
    await client.fs.directory.read({ path: null, ...target });
    await client.memory.get({ date: "2026-10-11", ...target });
    await client.memory.getEntry({ memoryId: "memory-1", ...target });
    await client.skills.refresh(target);
    await client.workspaceDocs.list(target);
    await client.workspaceDocs.get("soul", target);
    await client.workspaceDocs.put("soul", { content: "worker document", base_version: { mode: "must_not_exist" } }, target);
    expect(request.mock.calls.map(([url]) => url)).toEqual([
      "http://localhost:8080/v1/config/llm?nodeId=worker-b",
      "http://localhost:8080/v1/config/reload?nodeId=worker-b",
      "http://localhost:8080/v1/fs/directory?nodeId=worker-b",
      "http://localhost:8080/v1/memory/files/2026-10-11?nodeId=worker-b",
      "http://localhost:8080/v1/memory/entries/memory-1?nodeId=worker-b",
      "http://localhost:8080/v1/skills/refresh?nodeId=worker-b",
      "http://localhost:8080/v1/workspace/docs?nodeId=worker-b",
      "http://localhost:8080/v1/workspace/docs/soul?nodeId=worker-b",
      "http://localhost:8080/v1/workspace/docs/soul?nodeId=worker-b",
    ]);
  });

  it("reads and writes memory files through request objects", async () => {
    const request = vi.fn().mockResolvedValue({ file: { date: "2026-06-24" } });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.memory.get({ date: "2026-06-24" });
    await client.memory.put({
      date: "2026-06-24",
      content: "# Memory",
      expected_version: "memory-token",
    });

    expect(request).toHaveBeenNthCalledWith(
      1,
      "http://localhost:8080/v1/memory/files/2026-06-24",
    );
    expect(request).toHaveBeenNthCalledWith(
      2,
      "http://localhost:8080/v1/memory/files/2026-06-24",
      {
        method: "PUT",
        body: JSON.stringify({
          date: "2026-06-24",
          content: "# Memory",
          expected_version: "memory-token",
        }),
      },
    );
  });

  it("preserves the selected Computer across environment JSON-RPC methods", async () => {
    const transport = new RecordingTransport();
    const client = new AppServerClient(transport);
    await client.workspaceInfo({ nodeId: "worker-1" });
    await client.listModels({ nodeId: "worker-1" });
    await client.listProjects({ nodeId: "worker-1" });
    expect(transport.requests.map(({ method, params }) => ({ method, params }))).toEqual([
      { method: "workspace/info", params: { nodeId: "worker-1" } },
      { method: "model/list", params: { nodeId: "worker-1" } },
      { method: "project/list", params: { nodeId: "worker-1" } },
    ]);
  });

  it("passes Computer routing through document JSON-RPC without changing content/version", async () => {
    const transport = new RecordingTransport();
    const client = new AppServerClient(transport);
    const target = { nodeId: "worker-b" };
    await client.configGet("llm", target);
    await client.configReload(target);
    await client.readMemoryFile({ date: "2026-10-11", ...target });
    await client.refreshSkills(target);
    await client.listWorkspaceDocs(target);
    await client.readWorkspaceDoc("soul", target);
    await client.writeWorkspaceDoc("soul", "worker document", "version-1", target);
    expect(transport.requests).toEqual([
      { method: "config/get", params: { path: "llm", ...target } },
      { method: "config/reload", params: target },
      { method: "memory/file/read", params: { date: "2026-10-11", ...target } },
      { method: "skills/refresh", params: target },
      { method: "workspace/doc/list", params: target },
      { method: "workspace/doc/read", params: { kind: "soul", ...target } },
      { method: "workspace/doc/write", params: { kind: "soul", content: "worker document", expected_version: "version-1", ...target } },
    ]);
  });
});

function noopStreamAdapter(): StreamAdapter {
  return { stream: vi.fn().mockResolvedValue(undefined) };
}

it("exposes owner deletion with the same response over HTTP and JSON-RPC", async () => {
  const response = { nodeId: "worker/old" };
  const request = vi.fn().mockResolvedValue(response);
  const browser = new TalosClient({ request }, noopStreamAdapter(), "http://localhost:8080");
  await expect(browser.computers.delete(response)).resolves.toEqual(response);
  expect(request).toHaveBeenCalledExactlyOnceWith("http://localhost:8080/v1/computers/worker%2Fold", { method: "DELETE" });
  const transport = new RecordingTransport();
  vi.spyOn(transport, "request").mockResolvedValue(response);
  const app = new AppServerClient(transport);
  await expect(app.computers.delete(response)).resolves.toEqual(response);
  expect(transport.request).toHaveBeenCalledWith("computer/delete", response);
});
