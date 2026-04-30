import { describe, expect, it } from "vitest";
import type * as AppProtocol from "../generated/app-protocol";

import {
  AppServerClient,
  type AppServerEventHandler,
  type AppServerTransport,
} from "../app-server";

class RecordingTransport implements AppServerTransport {
  readonly requests: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];

  request<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    this.requests.push({ method, params });
    if (
      method === "model.list" ||
      method === "memory.search" ||
      method === "project.list" ||
      method === "session.turns"
    ) {
      return Promise.resolve({ data: [] } as T);
    }
    return Promise.resolve({} as T);
  }

  onEvent(handler: AppServerEventHandler): () => void {
    void handler;
    return () => undefined;
  }
}

describe("AppServerClient", () => {
  it("maps shared application methods to typed JSON-RPC methods", async () => {
    const transport = new RecordingTransport();
    const client = new AppServerClient(transport);

    await client.listModels();
    await client.patchSession({
      session_id: "session-1",
      project_id: null,
      current_directory: null,
      model: "MiniMax-M2.5",
    });
    await client.listProjects();
    await client.createProject("Talos", "/tmp/talos");
    await client.patchProject("project-1", { name: "Talos Core" });
    await client.getSessionTurns("session-1");
    await client.searchMemory("stable fact", 3);

    expect(transport.requests).toEqual([
      { method: "model.list", params: {} },
      {
        method: "session.patch",
        params: {
          session_id: "session-1",
          project_id: null,
          current_directory: null,
          model: "MiniMax-M2.5",
        },
      },
      { method: "project.list", params: {} },
      { method: "project.create", params: { name: "Talos", directory: "/tmp/talos" } },
      { method: "project.patch", params: { project_id: "project-1", name: "Talos Core" } },
      { method: "session.turns", params: { session_id: "session-1" } },
      { method: "memory.search", params: { query: "stable fact", limit: 3 } },
    ]);
  });
});
