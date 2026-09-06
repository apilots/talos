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
    if (method === "model/list") {
      return Promise.resolve({ models: [] } as T);
    }
    if (method === "memory/search") {
      return Promise.resolve({ data: [] } as T);
    }
    if (method === "project/list") {
      return Promise.resolve({ projects: [] } as T);
    }
    if (method === "timeline/recover") {
      return Promise.resolve(emptyRecoverySnapshot as T);
    }
    if (method === "config/get" || method === "config/update") {
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
  it("maps message queue management to the canonical JSON-RPC methods", async () => {
    const transport = new RecordingTransport();
    const client = new AppServerClient(transport);
    const input = { content: [{ type: "text" as const, text: "queued" }] };

    await client.messageQueue.add({ sessionId: "s1", input });
    await client.messageQueue.list({ sessionId: "s1" });
    await client.messageQueue.update({ sessionId: "s1", messageId: "q1", input });
    await client.messageQueue.delete({ sessionId: "s1", messageId: "q1" });
    await client.messageQueue.reorder({ sessionId: "s1", messageIds: ["q2", "q1"] });
    await client.messageQueue.start({ sessionId: "s1", messageId: null });

    expect(transport.requests).toEqual([
      { method: "messageQueue/add", params: { sessionId: "s1", input } },
      { method: "messageQueue/list", params: { sessionId: "s1" } },
      { method: "messageQueue/update", params: { sessionId: "s1", messageId: "q1", input } },
      { method: "messageQueue/delete", params: { sessionId: "s1", messageId: "q1" } },
      { method: "messageQueue/reorder", params: { sessionId: "s1", messageIds: ["q2", "q1"] } },
      { method: "messageQueue/start", params: { sessionId: "s1", messageId: null } },
    ]);
  });

  it("maps the files API to canonical App Server methods", async () => {
    const transport = new RecordingTransport();
    const client = new AppServerClient(transport);
    const request = {
      filename: "note.txt",
      scope: { type: "personal" as const },
      tags: [],
      metadata: {},
    };

    await client.files.upload(request, new Uint8Array([97, 98]));
    await client.files.list({ limit: 10 });
    await client.files.read({ fileId: "file-1" });
    await client.files.update({ fileId: "file-1", tags: [] });
    await client.files.delete({ fileId: "file-1" });

    expect(transport.requests).toEqual([
      {
        method: "file/upload",
        params: { request, dataBase64: "YWI=" },
      },
      { method: "file/list", params: { limit: 10 } },
      { method: "file/read", params: { fileId: "file-1" } },
      { method: "file/update", params: { fileId: "file-1", tags: [] } },
      { method: "file/delete", params: { fileId: "file-1" } },
    ]);
  });

  it("removes a completed desktop upload when cancellation wins the response race", async () => {
    const controller = new AbortController();
    const transport = new CancellingUploadTransport(controller);
    const client = new AppServerClient(transport);

    const upload = client.files.upload(
      {
        filename: "note.txt",
        scope: { type: "personal" },
        tags: [],
        metadata: {},
      },
      new Uint8Array([97, 98]),
      { signal: controller.signal },
    );

    await expect(upload).rejects.toMatchObject({ name: "AbortError" });
    expect(transport.requests.map(({ method }) => method)).toEqual([
      "file/upload",
      "file/delete",
    ]);
    expect(transport.requests[1]?.params).toEqual({ fileId: "file-1" });
  });

  it("maps shared application methods to typed JSON-RPC methods", async () => {
    const transport = new RecordingTransport();
    const client = new AppServerClient(transport);

    await client.listModels();
    await client.readProfile();
    await client.patchSession({
      sessionId: "session-1",
      conf: {
        userPermissionMode: null,
        modelSelection: {
          modelRef: { provider: "bailian", id: "MiniMax-M2.5" },
          reasoningEffort: null,
        },
      },
      currentDirectory: null,
    });
    await client.listProjects({});
    await client.getLlmConfig();
    await client.updateLlmConfig(testLlmConfig);
    await client.configDelete("webSearch");
    await client.createProject({ name: "Talos", directory: "/tmp/talos" });
    await client.patchProject({ project_id: "project-1", name: "Talos Core" });
    await client.recoverTimeline({
      sessionId: "session-1",
      afterEventId: "ev_00000000000000000001",
      replayLimit: 100,
    });
    await client.submitTurnInput({
      identity: { sessionId: "session-1", turnId: "turn-1" },
      input: {
        type: "userActionResponse",
        actionId: "action-1",
        response: {
          type: "question",
          response: { type: "selectedOption", optionId: "continue" },
        },
      },
      clientRequestId: null,
    });
    await client.startCloudLogin({ client_name: "Talos Test", client_kind: "web" });
    await client.getCloudLoginStatus({ login_id: "login-1" });
    await client.cancelCloudLogin({ login_id: "login-1" });
    await client.unsubscribeEvents("sub-1");
    await client.readMemoryFile({ date: "2026-06-24" });
    await client.writeMemoryFile({
      date: "2026-06-24",
      content: "# Memory",
      expected_version: "memory-token",
    });
    await client.searchMemory({ query: "stable fact", limit: 3 });

    expect(transport.requests).toEqual([
      { method: "model/list", params: {} },
      { method: "profile/read", params: {} },
      {
        method: "session/patch",
        params: {
          sessionId: "session-1",
          conf: {
            userPermissionMode: null,
            modelSelection: {
              modelRef: { provider: "bailian", id: "MiniMax-M2.5" },
              reasoningEffort: null,
            },
          },
          currentDirectory: null,
        },
      },
      { method: "project/list", params: {} },
      { method: "config/get", params: { path: "llm" } },
      {
        method: "config/update",
        params: {
          value: { type: "llm", value: testLlmConfig },
        },
      },
      { method: "config/delete", params: { path: "webSearch" } },
      { method: "project/create", params: { name: "Talos", directory: "/tmp/talos" } },
      { method: "project/patch", params: { project_id: "project-1", name: "Talos Core" } },
      {
        method: "timeline/recover",
        params: {
          sessionId: "session-1",
          afterEventId: "ev_00000000000000000001",
          replayLimit: 100,
        },
      },
      {
        method: "turn/input",
        params: {
          identity: { sessionId: "session-1", turnId: "turn-1" },
          input: {
            type: "userActionResponse",
            actionId: "action-1",
            response: {
              type: "question",
              response: { type: "selectedOption", optionId: "continue" },
            },
          },
          clientRequestId: null,
        },
      },
      {
        method: "auth/login/start",
        params: { client_name: "Talos Test", client_kind: "web" },
      },
      { method: "auth/login/status", params: { login_id: "login-1" } },
      { method: "auth/login/cancel", params: { login_id: "login-1" } },
      { method: "event/unsubscribe", params: { subscriptionId: "sub-1" } },
      { method: "memory/file/read", params: { date: "2026-06-24" } },
      {
        method: "memory/file/write",
        params: {
          date: "2026-06-24",
          content: "# Memory",
          expected_version: "memory-token",
        },
      },
      { method: "memory/search", params: { query: "stable fact", limit: 3 } },
    ]);
  });
});

class CancellingUploadTransport extends RecordingTransport {
  constructor(private readonly controller: AbortController) {
    super();
  }

  override request<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    this.requests.push({ method, params });
    if (method === "file/upload") {
      this.controller.abort();
      return Promise.resolve({
        file: { fileId: "file-1" },
      } as T);
    }
    return Promise.resolve({ fileId: "file-1" } as T);
  }
}

const emptyProvider = {
  models: [],
};

const emptyRecoverySnapshot = {
  replay: {
    events: [],
    highWaterMarkEventId: null,
    nextCursor: null,
    lastEventSeq: 0,
    cursorStatus: "fromBeginning",
  },
  activeTurn: null,
};

const testLlmConfig = {
  provider: "anthropic",
  model: "MiniMax-M2.5",
  anthropic: emptyProvider,
  openai: emptyProvider,
  bailian: emptyProvider,
  deepseek: emptyProvider,
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
  provider_capabilities: {
    anthropic: { default_protocol: "messages", protocols: ["messages"] },
    openai: { default_protocol: "chat", protocols: ["chat", "responses"] },
    bailian: { default_protocol: "chat", protocols: ["chat"] },
    deepseek: { default_protocol: "chat", protocols: ["chat", "messages"] },
    ollama: { default_protocol: "chat", protocols: ["chat"] },
    openrouter: {
      default_protocol: "chat",
      protocols: ["chat", "responses", "messages"],
    },
  },
};
