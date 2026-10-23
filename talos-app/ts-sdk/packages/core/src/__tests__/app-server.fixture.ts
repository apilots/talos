import type * as AppProtocol from "../generated/app-protocol";
import type { AppServerTransport, AppServerEventHandler } from "../app-server";

export class RecordingTransport implements AppServerTransport {
  readonly requests: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];

  request<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    this.requests.push({ method, params });
    if (method === "file/content") {
      return Promise.resolve({ file: { fileId: "file-1" }, dataBase64: "YWI=" } as T);
    }
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

export const testLlmConfig = {
  custom_providers: {},
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
