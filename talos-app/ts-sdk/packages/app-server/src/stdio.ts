import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { EventEmitter } from "node:events";
import { createInterface, type Interface as ReadlineInterface } from "node:readline";

import type { AppProtocol } from "@talos/sdk-core";

import {
  AppServerClient,
  type AppServerEventHandler,
  type AppServerTransport,
} from "@talos/sdk-core";

export interface JsonRpcStdioTransportOptions {
  command?: string;
  args?: string[];
  env?: NodeJS.ProcessEnv;
  cwd?: string;
}

export interface AppServerClientOptions extends JsonRpcStdioTransportOptions {
  launchToken: string;
  clientName?: string;
  clientVersion?: string;
}

export type AppServerNotificationHandler = (
  notification: AppProtocol.JsonRpcNotification,
) => void;

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
}

export class AppServerProtocolError extends Error {
  readonly code: number;
  readonly data: AppProtocol.AppServerErrorData | null;

  constructor(error: AppProtocol.AppServerErrorBody) {
    super(error.message);
    this.name = "AppServerProtocolError";
    this.code = error.code;
    this.data = error.data;
  }
}

export class JsonRpcStdioTransport implements AppServerTransport {
  private child?: ChildProcessWithoutNullStreams;
  private stdout?: ReadlineInterface;
  private nextId = 1;
  private readonly pending = new Map<AppProtocol.RequestId, PendingRequest>();
  private readonly events = new EventEmitter();

  constructor(private readonly options: JsonRpcStdioTransportOptions = {}) {}

  start(): void {
    if (this.child) {
      return;
    }

    const child = spawn(this.options.command ?? "talos", this.options.args ?? ["app-server"], {
      cwd: this.options.cwd,
      env: { ...process.env, ...(this.options.env ?? {}) },
      stdio: ["pipe", "pipe", "pipe"],
    });

    child.stderr.on("data", (chunk: Buffer) => {
      this.events.emit("stderr", chunk.toString("utf8"));
    });
    child.once("error", (error) => {
      this.rejectPending(error);
      this.events.emit("error", error);
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
    const child = this.child;
    if (!child) {
      return;
    }

    this.child = undefined;
    this.stdout?.close();
    this.stdout = undefined;
    child.stdin.end();
    await new Promise<void>((resolve) => {
      child.once("exit", () => resolve());
      child.kill("SIGTERM");
      setTimeout(resolve, 2_000).unref();
    });
  }

  terminate(signal: NodeJS.Signals = "SIGTERM"): boolean {
    const child = this.child;
    if (!child) {
      return false;
    }
    return child.kill(signal);
  }

  request<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
    this.start();
    const child = this.requireChild();
    const id = this.nextId++;
    const message: AppProtocol.JsonRpcRequest = {
      jsonrpc: "2.0",
      id,
      method,
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

  onStderr(handler: (chunk: string) => void): () => void {
    this.events.on("stderr", handler);
    return () => this.events.off("stderr", handler);
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
      message = JSON.parse(trimmed) as AppProtocol.JsonRpcMessage;
    } catch (error) {
      this.events.emit("error", error instanceof Error ? error : new Error("Invalid JSON-RPC line."));
      return;
    }

    if ("error" in message) {
      const pending = message.id === null ? undefined : this.pending.get(message.id);
      if (pending) {
        this.pending.delete(message.id as AppProtocol.RequestId);
        pending.reject(new AppServerProtocolError(message.error));
      }
      return;
    }

    if ("result" in message && "id" in message) {
      const pending = this.pending.get(message.id);
      if (pending) {
        this.pending.delete(message.id);
        pending.resolve(message.result);
      }
      return;
    }

    if ("method" in message && !("id" in message)) {
      this.events.emit("notification", message);
      if (isApplicationEventEnvelope(message.params)) {
        this.events.emit("event", message.params);
      }
      return;
    }

    if ("method" in message && "id" in message) {
      this.writeError(message.id, -32601, `unsupported server request: ${message.method}`);
    }
  }

  private writeError(id: AppProtocol.RequestId, code: number, message: string): void {
    const response: AppProtocol.JsonRpcErrorResponse = {
      jsonrpc: "2.0",
      id,
      error: {
        code,
        message,
        data: null,
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

  constructor(private readonly options: AppServerClientOptions) {
    const transport = new JsonRpcStdioTransport({
      command: options.command,
      args: options.args ?? ["app-server"],
      cwd: options.cwd,
      env: {
        ...(options.env ?? {}),
        TALOS_LOCAL_LAUNCH_TOKEN: options.launchToken,
      },
    });
    super(transport);
    this.transport = transport;
  }

  async connect(): Promise<AppProtocol.InitializeResponse> {
    const response = await this.transport.request<AppProtocol.InitializeResponse>("initialize", {
      client_info: {
        name: this.options.clientName ?? "talos-ts-app-server-sdk",
        title: null,
        version: this.options.clientVersion ?? "0.1.0",
      },
      capabilities: {
        protocol_version: { major: 0, minor: 2 },
        supports_server_requests: false,
        supports_event_replay: true,
      },
      actor_credentials: {
        credential: {
          type: "local_launch_token",
          token: this.options.launchToken,
        },
      },
    } satisfies AppProtocol.InitializeParams);
    this.transport.notify("initialized", null);
    return response;
  }

  close(): Promise<void> {
    return this.transport.close();
  }
}

export function createStdioAppServerClient(options: AppServerClientOptions) {
  return new StdioAppServerClient(options);
}

function isApplicationEventEnvelope(value: unknown): value is AppProtocol.ApplicationEventEnvelope {
  if (!value || typeof value !== "object") {
    return false;
  }
  const candidate = value as Partial<AppProtocol.ApplicationEventEnvelope>;
  return (
    typeof candidate.subscription_id === "string" &&
    typeof candidate.event_id === "string" &&
    typeof candidate.event === "object" &&
    candidate.event !== null
  );
}
