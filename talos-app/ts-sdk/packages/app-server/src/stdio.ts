import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { EventEmitter } from "node:events";
import { createInterface, type Interface as ReadlineInterface } from "node:readline";

import {
  AppProtocol,
  AppServerClient,
  type AppServerErrorHandler,
  type AppServerEventHandler,
  type AppServerActiveTurnChangedHandler,
  type AppServerMessageQueueChangedHandler,
  type AppServerRequestOptions,
  type AppServerSubscriptionTerminatedHandler,
  type AppServerTransport,
  unwrapTalosApiResponse,
  parseTimelineEvent,
  parseTalosJson,
} from "@talos/sdk-core";

const PASSTHROUGH_ENV_KEYS = new Set(["HOME", "PATH", "SHELL", "TMPDIR", "USER"]);
const APP_SERVER_ERROR_CODE_VALUES = new Set<number>(
  Object.values(AppProtocol.AppServerErrorCode),
);

export interface JsonRpcStdioTransportOptions {
  command?: string;
  args?: string[];
  env?: NodeJS.ProcessEnv;
  cwd?: string;
  getActorCredentials?: () => AppProtocol.ActorCredentials | null | undefined;
}

export interface AppServerClientOptions extends JsonRpcStdioTransportOptions {
  clientName?: string;
  clientVersion?: string;
  clientKind?: AppProtocol.ClientKind;
  getActorCredentials?: () => AppProtocol.ActorCredentials | null | undefined;
}

export type AppServerNotificationHandler = (
  notification: AppProtocol.JsonRpcNotification,
) => void;

export type AppServerRequestHandler = (
  request: AppProtocol.JsonRpcRequest,
) => Promise<unknown> | unknown;

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
}

export class AppServerProtocolError extends Error {
  readonly code: AppProtocol.AppServerErrorCode;

  constructor(error: AppProtocol.AppServerErrorBody) {
    super(error.message);
    this.name = "AppServerProtocolError";
    this.code = error.code;
  }
}

export class JsonRpcStdioTransport implements AppServerTransport {
  private child?: ChildProcessWithoutNullStreams;
  private stdout?: ReadlineInterface;
  private closing?: Promise<void>;
  private nextId = 1;
  private readonly pending = new Map<AppProtocol.RequestId, PendingRequest>();
  private readonly events = new EventEmitter();
  private serverRequestHandler?: AppServerRequestHandler;

  constructor(private readonly options: JsonRpcStdioTransportOptions = {}) {}

  start(): void {
    if (this.closing) { throw new Error("App-server is closing."); }
    if (this.child) {
      return;
    }

    const child = spawn(this.options.command ?? "app-server", this.options.args ?? [], {
      cwd: this.options.cwd,
      env: buildAppServerEnvironment(process.env, this.options.env),
      stdio: ["pipe", "pipe", "pipe"],
    });

    child.stdin.on("error", (error) => {
      this.rejectPending(error);
      if (!this.closing) { this.events.emit("transportError", error); }
    });
    child.stderr.on("data", (chunk: Buffer) => {
      this.events.emit("stderr", chunk.toString("utf8"));
    });
    child.once("error", (error) => {
      this.rejectPending(error);
      this.events.emit("transportError", error);
    });
    child.once("exit", (code, signal) => {
      this.child = undefined;
      this.stdout?.close();
      this.stdout = undefined;
      this.rejectPending(
        new Error(`app-server exited: code=${code ?? "null"} signal=${signal ?? "null"}`),
      );
      this.events.emit("exit", { code, signal });
    });

    const stdout = createInterface({ input: child.stdout });
    stdout.on("line", (line) => this.handleLine(line));

    this.child = child;
    this.stdout = stdout;
  }

  async restart(): Promise<void> {
    await this.close();
    this.start();
  }

  async close(): Promise<void> {
    if (this.closing) { return this.closing; }
    const child = this.child;
    if (!child) { return; }
    this.rejectPending(new Error("App-server connection is closing."));
    // EOF lets the backend unregister the Worker and drain MCP before exiting.
    // The timeout covers the existing server shutdown budget, not a second lifecycle protocol.
    this.closing = new Promise<void>((resolve, reject) => {
      let forced = false;
      let killTimer: ReturnType<typeof setTimeout> | undefined;
      const timer = setTimeout(() => {
        forced = true;
        child.kill("SIGTERM");
        killTimer = setTimeout(() => child.kill("SIGKILL"), 2_000);
      }, 240_000);
      const finish = (error?: Error) => {
        clearTimeout(timer);
        if (killTimer) { clearTimeout(killTimer); }
        if (error) { reject(error); }
        else if (forced) { reject(new Error("App-server graceful shutdown timed out.")); }
        else { resolve(); }
      };
      child.once("exit", (code, signal) => finish(code === 0 && signal === null ? undefined :
        new Error("App-server exited before graceful shutdown completed.", { cause: { code, signal } })));
      child.once("error", finish);
      child.stdin.end();
    });
    try { await this.closing; }
    finally {
      this.stdout?.close();
      this.stdout = undefined;
      this.child = undefined;
      this.closing = undefined;
    }
  }

  terminate(signal: NodeJS.Signals = "SIGTERM"): boolean {
    const child = this.child;
    if (!child) {
      return false;
    }
    return child.kill(signal);
  }

  request<T>(
    method: AppProtocol.ClientMethod,
    params: unknown,
    options?: AppServerRequestOptions,
  ): Promise<T> {
    this.start();
    const child = this.requireChild();
    const id = this.nextId++;
    const actorCredentials = options?.actorCredentials ?? this.options.getActorCredentials?.();
    const message: AppProtocol.JsonRpcRequest = {
      jsonrpc: "2.0",
      id,
      method,
      actor_credentials: actorCredentials ?? null,
      params,
    };

    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, {
        resolve: (value) => resolve(value as T),
        reject,
      });
      child.stdin.write(`${JSON.stringify(message)}\n`, (error) => {
        if (!error) {
          return;
        }
        this.pending.delete(id);
        reject(error);
      });
    });
  }

  notify(method: string, params?: unknown): void {
    this.start();
    const message: AppProtocol.JsonRpcNotification = {
      jsonrpc: "2.0",
      method,
      params,
    };
    this.requireChild().stdin.write(`${JSON.stringify(message)}\n`);
  }

  onNotification(handler: AppServerNotificationHandler): () => void {
    this.events.on("notification", handler);
    return () => this.events.off("notification", handler);
  }

  onEvent(handler: AppServerEventHandler): () => void {
    this.events.on("event", handler);
    return () => this.events.off("event", handler);
  }

  onActiveTurnChanged(handler: AppServerActiveTurnChangedHandler): () => void {
    this.events.on("activeTurnChanged", handler); return () => this.events.off("activeTurnChanged", handler);
  }

  onMessageQueueChanged(handler: AppServerMessageQueueChangedHandler): () => void {
    this.events.on("messageQueueChanged", handler);
    return () => this.events.off("messageQueueChanged", handler);
  }

  onSubscriptionTerminated(handler: AppServerSubscriptionTerminatedHandler): () => void {
    this.events.on("subscriptionTerminated", handler);
    return () => this.events.off("subscriptionTerminated", handler);
  }

  onError(handler: AppServerErrorHandler): () => void {
    this.events.on("transportError", handler);
    return () => this.events.off("transportError", handler);
  }

  onStderr(handler: (chunk: string) => void): () => void {
    this.events.on("stderr", handler);
    return () => this.events.off("stderr", handler);
  }

  onServerRequest(handler: AppServerRequestHandler): () => void {
    this.serverRequestHandler = handler;
    return () => {
      if (this.serverRequestHandler === handler) {
        this.serverRequestHandler = undefined;
      }
    };
  }

  onExit(
    handler: (status: { code: number | null; signal: NodeJS.Signals | null }) => void,
  ): () => void {
    this.events.on("exit", handler);
    return () => this.events.off("exit", handler);
  }

  private handleLine(line: string): void {
    const trimmed = line.trim();
    if (!trimmed) {
      return;
    }

    let message: AppProtocol.JsonRpcMessage;
    try {
      message = parseTalosJson(trimmed) as AppProtocol.JsonRpcMessage;
      if (!message || typeof message !== "object" || Array.isArray(message) ||
          ("method" in message && typeof message.method !== "string")) {
        throw new Error("Invalid JSON-RPC message.");
      }
    } catch (error) {
      const failure = error instanceof Error ? error : new Error("Invalid JSON-RPC line.");
      this.rejectPending(failure);
      this.events.emit("transportError", failure);
      return;
    }

    if ("error" in message) {
      const pending = message.id === null ? undefined : this.pending.get(message.id);
      let protocolError: AppProtocol.AppServerErrorBody;
      try {
        protocolError = decodeAppServerErrorBody(message.error);
      } catch (error) {
        const decodeError =
          error instanceof Error ? error : new Error("Invalid app-server protocol error.");
        if (pending) {
          this.pending.delete(message.id as AppProtocol.RequestId);
          pending.reject(decodeError);
        } else {
          this.rejectPending(decodeError);
          this.events.emit("transportError", decodeError);
        }
        return;
      }
      const failure = new AppServerProtocolError(protocolError);
      if (pending) {
        this.pending.delete(message.id as AppProtocol.RequestId);
        pending.reject(failure);
      } else {
        this.rejectPending(failure);
        this.events.emit("transportError", failure);
      }
      return;
    }

    if ("result" in message && "id" in message) {
      const pending = this.pending.get(message.id);
      if (pending) {
        this.pending.delete(message.id);
        try {
          pending.resolve(unwrapAppServerResult(message.result));
        } catch (error) {
          pending.reject(error instanceof Error ? error : new Error("Invalid app-server result."));
        }
      }
      return;
    }

    if ("method" in message && !("id" in message)) {
      if (message.method.startsWith("timeline/") && !isTimelineEventNotification(message.params)) {
        this.events.emit("transportError", new Error("Invalid Talos timeline event."));
        return;
      }
      this.events.emit("notification", message);
      if (isTimelineEventNotification(message.params)) {
        try {
          this.events.emit("event", parseTimelineEvent(message.params.event));
        } catch (error) {
          // The renderer may receive events before its subscribe promise has settled.
          this.events.emit("subscriptionTerminated", {
            subscriptionId: message.params.subscriptionId,
            sessionId: message.params.event.event.data.identity.sessionId,
            reason: "interrupted",
            message: "The app-server returned an invalid timeline event.",
          } satisfies AppProtocol.SubscriptionTerminatedNotification);
          this.events.emit("transportError", error instanceof Error ? error : new Error("Invalid Talos timeline event."));
        }
      }
      if (isSubscriptionTerminatedNotification(message.params)) {
        this.events.emit("subscriptionTerminated", message.params);
      }
      if (message.method === "session/activeTurnChanged" && message.params && typeof message.params === "object" && "sessionId" in message.params && "state" in message.params) {
        this.events.emit("activeTurnChanged", message.params);
      }
      if (
        message.method === "messageQueue/changed" &&
        isMessageQueueChangedNotification(message.params)
      ) {
        this.events.emit("messageQueueChanged", message.params);
      }
      return;
    }

    if ("method" in message && "id" in message) {
      void this.handleServerRequest(message);
    }
  }

  private async handleServerRequest(request: AppProtocol.JsonRpcRequest): Promise<void> {
    const handler = this.serverRequestHandler;
    if (!handler) {
      this.writeError(
        request.id,
        AppProtocol.AppServerErrorCode.MethodNotFound,
        `unsupported server request: ${request.method}`,
      );
      return;
    }

    try {
      const result = await handler(request);
      this.writeResult(request.id, result ?? null);
    } catch (error) {
      this.writeError(
        request.id,
        AppProtocol.AppServerErrorCode.InternalError,
        errorMessage(error),
      );
    }
  }

  private writeResult(id: AppProtocol.RequestId, result: unknown): void {
    const response: AppProtocol.JsonRpcResponse = {
      jsonrpc: "2.0",
      id,
      result,
    };
    this.requireChild().stdin.write(`${JSON.stringify(response)}\n`);
  }

  private writeError(
    id: AppProtocol.RequestId,
    code: AppProtocol.AppServerErrorCode,
    message: string,
  ): void {
    const response: AppProtocol.JsonRpcErrorResponse = {
      jsonrpc: "2.0",
      id,
      error: {
        code,
        message,
      },
    };
    this.requireChild().stdin.write(`${JSON.stringify(response)}\n`);
  }

  private rejectPending(error: Error): void {
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }

  private requireChild(): ChildProcessWithoutNullStreams {
    if (!this.child) {
      throw new Error("app-server process is not running.");
    }
    return this.child;
  }
}

export class StdioAppServerClient extends AppServerClient {
  readonly transport: JsonRpcStdioTransport;
  private initialization?: Promise<AppProtocol.InitializeResponse>;

  constructor(private readonly options: AppServerClientOptions) {
    const transport = new JsonRpcStdioTransport({
      command: options.command,
      args: options.args ?? [],
      cwd: options.cwd,
      getActorCredentials: options.getActorCredentials,
      env: options.env,
    });
    super(transport);
    this.transport = transport;
    transport.onExit(() => { this.initialization = undefined; });
  }

  connect(): Promise<AppProtocol.InitializeResponse> {
    this.initialization ??= this.initialize().catch((error: unknown) => {
      this.initialization = undefined;
      throw error;
    });
    return this.initialization;
  }

  private async initialize(): Promise<AppProtocol.InitializeResponse> {
    const response = await this.transport.request<AppProtocol.InitializeResponse>("initialize", {
      client_info: {
        name: this.options.clientName ?? "talos-ts-app-server-sdk",
        title: null,
        version: this.options.clientVersion ?? "0.1.14-SNAPSHOT",
        kind: this.options.clientKind ?? "sdk",
      },
      capabilities: {
        protocol_version: AppProtocol.PROTOCOL_VERSION,
        supports_server_requests: true,
        supports_event_replay: true,
      },
      actor_credentials: null,
    } satisfies AppProtocol.InitializeParams);
    if (response.protocol_version.major !== AppProtocol.PROTOCOL_VERSION.major ||
        response.protocol_version.minor !== AppProtocol.PROTOCOL_VERSION.minor) {
      throw new Error("Unsupported app-server protocol version; update the backend and client together.");
    }
    this.transport.notify("initialized", null);
    return response;
  }

  close(): Promise<void> {
    this.initialization = undefined;
    return this.transport.close();
  }

}

export function createStdioAppServerClient(options: AppServerClientOptions) {
  return new StdioAppServerClient(options);
}

export function filterAppServerEnvironment(baseEnv: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const [key, value] of Object.entries(baseEnv)) {
    if (value === undefined) {
      continue;
    }
    if (key.startsWith("TALOS_") || PASSTHROUGH_ENV_KEYS.has(key)) {
      env[key] = value;
    }
  }
  return env;
}

function buildAppServerEnvironment(
  baseEnv: NodeJS.ProcessEnv,
  overrideEnv?: NodeJS.ProcessEnv,
): NodeJS.ProcessEnv {
  return {
    ...filterAppServerEnvironment(baseEnv),
    ...filterAppServerEnvironment(overrideEnv ?? {}),
  };
}

function isTimelineEventNotification(
  value: unknown,
): value is AppProtocol.TimelineEventNotification {
  if (!value || typeof value !== "object") {
    return false;
  }
  const candidate = value as Partial<AppProtocol.TimelineEventNotification>;
  const event = candidate.event as Partial<AppProtocol.TimelineEvent> | undefined;
  const payload = event?.event;
  const identity = payload?.data?.identity;
  return (
    typeof candidate.subscriptionId === "string" &&
    !!event &&
    typeof event.eventId === "string" &&
    typeof event.eventSeq === "number" &&
    typeof event.persistedAt === "number" &&
    !!identity &&
    typeof identity.sessionId === "string" &&
    typeof identity.turnId === "string"
  );
}

function isSubscriptionTerminatedNotification(value: unknown): value is AppProtocol.SubscriptionTerminatedNotification {
  if (!value || typeof value !== "object") {
    return false;
  }
  const candidate = value as Partial<AppProtocol.SubscriptionTerminatedNotification>;
  return (
    typeof candidate.subscriptionId === "string" &&
    typeof candidate.sessionId === "string" &&
    typeof candidate.reason === "string" &&
    typeof candidate.message === "string"
  );
}

function isMessageQueueChangedNotification(
  value: unknown,
): value is AppProtocol.MessageQueueChangedNotification {
  if (!value || typeof value !== "object") {
    return false;
  }
  const candidate = value as Partial<AppProtocol.MessageQueueChangedNotification>;
  return typeof candidate.sessionId === "string" && typeof candidate.revision === "number";
}

function unwrapAppServerResult(value: unknown): unknown {
  return unwrapTalosApiResponse(value);
}

function decodeAppServerErrorBody(value: unknown): AppProtocol.AppServerErrorBody {
  if (!value || typeof value !== "object") {
    throw new Error("Invalid app-server protocol error.");
  }
  const error = value as Record<string, unknown>;
  if (
    typeof error.code !== "number" ||
    !APP_SERVER_ERROR_CODE_VALUES.has(error.code) ||
    typeof error.message !== "string"
  ) {
    throw new Error("Invalid app-server protocol error.");
  }
  return {
    code: error.code as AppProtocol.AppServerErrorCode,
    message: error.message,
  };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
