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
    if (method === "model.list") {
      return Promise.resolve({ models: [] } as T);
    }
    if (method === "memory.search") {
      return Promise.resolve({ data: [] } as T);
    }
    if (method === "project.list") {
      return Promise.resolve({ projects: [] } as T);
    }
    if (method === "session.recover") {
      return Promise.resolve({ snapshot: emptyRecoverySnapshot } as T);
    }
    if (method === "config.get" || method === "config.update") {
      return Promise.resolve(testLlmConfigRead as T);
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
      conf: { agent: { model: "MiniMax-M2.5" } },
      project_id: null,
      current_directory: null,
    });
    await client.listProjects({});
    await client.getLlmConfig();
    await client.updateLlmConfig(testLlmConfig);
    await client.createProject({ name: "Talos", directory: "/tmp/talos" });
    await client.patchProject({ project_id: "project-1", name: "Talos Core" });
    await client.recoverSession({
      session_id: "session-1",
      after_event_id: "ev_00000000000000000001",
      replay_limit: 100,
    });
    await client.submitDynamicToolResult({
      session_id: "session-1",
      call_id: "call-1",
      tool_name: "DynamicTool",
      output: "done",
      success: true,
    });
    await client.startCloudLogin({ client_name: "Talos Test", client_kind: "web" });
    await client.getCloudLoginStatus({ login_id: "login-1" });
    await client.cancelCloudLogin({ login_id: "login-1" });
    await client.unsubscribeEvents("sub-1");
    await client.searchMemory("stable fact", 3);

    expect(transport.requests).toEqual([
      { method: "model.list", params: {} },
      {
        method: "session.patch",
        params: {
          session_id: "session-1",
          conf: { agent: { model: "MiniMax-M2.5" } },
          project_id: null,
          current_directory: null,
        },
      },
      { method: "project.list", params: {} },
      { method: "config.get", params: {} },
      {
        method: "config.update",
        params: {
          path: "llm",
          value: { type: "llm", value: testLlmConfig },
        },
      },
      { method: "project.create", params: { name: "Talos", directory: "/tmp/talos" } },
      { method: "project.patch", params: { project_id: "project-1", name: "Talos Core" } },
      {
        method: "session.recover",
        params: {
          session_id: "session-1",
          after_event_id: "ev_00000000000000000001",
          replay_limit: 100,
        },
      },
      {
        method: "tool.dynamic_result.submit",
        params: {
          session_id: "session-1",
          call_id: "call-1",
          tool_name: "DynamicTool",
          output: "done",
          success: true,
        },
      },
      {
        method: "auth.login.start",
        params: { client_name: "Talos Test", client_kind: "web" },
      },
      { method: "auth.login.status", params: { login_id: "login-1" } },
      { method: "auth.login.cancel", params: { login_id: "login-1" } },
      { method: "event.unsubscribe", params: { subscription_id: "sub-1" } },
      { method: "memory.search", params: { query: "stable fact", limit: 3 } },
    ]);
  });
});

const emptyProvider = {
  enabled: false,
  models: [],
};

const emptyRecoverySnapshot = {
  replay: {
    events: [],
    high_water_mark_event_id: null,
    last_replay_seq: 0,
    cursor_status: "from_beginning",
  },
  active_turn: {
    session_id: "session-1",
    turn_id: null,
    message_id: null,
    status: "completed",
    last_event_id: null,
    pending_user_action: null,
    updated_at: 0,
  },
};

const testLlmConfig = {
  provider: "anthropic",
  model: "MiniMax-M2.5",
  anthropic: emptyProvider,
  openai: emptyProvider,
  bailian: emptyProvider,
  ollama: emptyProvider,
  openrouter: emptyProvider,
};

const testLlmConfigRead = {
  config: testLlmConfig,
  model_readiness: {
    ready: true,
    provider: "anthropic",
    model: "MiniMax-M2.5",
    issues: [],
  },
  provider_keys: {},
};
