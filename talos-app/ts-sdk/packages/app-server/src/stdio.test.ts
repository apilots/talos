import { describe, expect, it, vi } from "vitest";

import { AppProtocol, TalosApiError } from "@talos/sdk-core";

import { AppServerProtocolError, JsonRpcStdioTransport, StdioAppServerClient, filterAppServerEnvironment } from "./stdio";

it("shares concurrent initialization and does not send initialize twice", async () => {
  const client = new StdioAppServerClient({});
  const response = { protocol_version: AppProtocol.PROTOCOL_VERSION,
    server_info: { name: "fixture", version: "test", auth_enabled: false, cloud_auth_url: null } };
  const request = vi.spyOn(client.transport, "request").mockResolvedValue(response);
  vi.spyOn(client.transport, "notify").mockImplementation(() => undefined);
  expect(await Promise.all([client.connect(), client.connect(), client.connect()])).toEqual([response, response, response]);
  expect(request).toHaveBeenCalledTimes(1);
});

describe("AppServerProtocolError", () => {
  it("preserves protocol error fields", () => {
    const error = new AppServerProtocolError({
      code: AppProtocol.AppServerErrorCode.AlreadyInitialized,
      message: "missing session",
    });

    expect(error.name).toBe("AppServerProtocolError");
    expect(error.message).toBe("missing session");
    expect(error.code).toBe(-32002);
  });
});

describe("JsonRpcStdioTransport server requests", () => {
  it("responds to server requests with registered handler output", async () => {
    const transport = createTransportHarness();
    transport.instance.onServerRequest((request) => {
      expect(request.method).toBe("client/user_input/request");
      return { text: "confirmed" };
    });

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      id: 7,
      method: "client/user_input/request",
      params: { prompt: "Continue?" },
    }));
    await Promise.resolve();

    expect(transport.writes.map((line) => JSON.parse(line))).toEqual([
      {
        jsonrpc: "2.0",
        id: 7,
        result: { text: "confirmed" },
      },
    ]);
  });

  it("returns method-not-found when no server request handler is registered", async () => {
    const transport = createTransportHarness();

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      id: 9,
      method: "client/unknown",
      params: null,
    }));
    await Promise.resolve();

    expect(transport.writes.map((line) => JSON.parse(line))).toEqual([
      {
        jsonrpc: "2.0",
        id: 9,
        error: {
          code: AppProtocol.AppServerErrorCode.MethodNotFound,
          message: "unsupported server request: client/unknown",
        },
      },
    ]);
  });

  it("returns the registered internal protocol code when a server request handler fails", async () => {
    const transport = createTransportHarness();
    transport.instance.onServerRequest(() => {
      throw new Error("handler failed");
    });

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      id: 10,
      method: "client/user_input/request",
      params: { prompt: "Continue?" },
    }));
    await Promise.resolve();

    expect(transport.writes.map((line) => JSON.parse(line))).toEqual([
      {
        jsonrpc: "2.0",
        id: 10,
        error: {
          code: AppProtocol.AppServerErrorCode.InternalError,
          message: "handler failed",
        },
      },
    ]);
  });
});

describe("JsonRpcStdioTransport responses", () => {
  it.each([
    { id: null, code: AppProtocol.AppServerErrorCode.ParseError },
    { id: null, code: -99999 },
    { id: 99, code: AppProtocol.AppServerErrorCode.ParseError },
    { id: 99, code: -99999 },
  ])("rejects pending calls for an uncorrelated protocol error: $id/$code", async ({ id, code }) => {
    const transport = createTransportHarness();
    const reports: Error[] = [];
    const failures: unknown[] = [];
    transport.instance.onError((error) => reports.push(error));
    const requests = [
      transport.instance.request("initialize", {}),
      transport.instance.request("session/list", {}),
    ].map((request) => request.catch((error: unknown) => failures.push(error)));

    transport.handleLine(JSON.stringify({ jsonrpc: "2.0", id, error: { code, message: "The request could not be decoded." } }));
    await Promise.resolve();

    expect(reports).toHaveLength(1);
    expect(failures).toEqual([reports[0], reports[0]]);
    await Promise.all(requests);
  });

  it("unwraps successful API result data", async () => {
    const transport = createTransportHarness();
    const request = transport.instance.request<{ ok: boolean }>("session/get", {
      sessionId: "session-1",
    });

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      result: {
        status: "success",
        data: { ok: true },
      },
    }));

    await expect(request).resolves.toEqual({ ok: true });
  });

  it("rejects application API errors separately from protocol errors", async () => {
    const transport = createTransportHarness();
    const request = transport.instance.request("session/get", {
      sessionId: "missing",
    });

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      result: {
        status: "error",
        error: {
          code: AppProtocol.ErrorCode.ResourceNotFound, localization: null,
          message: "Resource not found",
        },
      },
    }));

    await expect(request).rejects.toMatchObject({
      name: "TalosApiError",
      code: AppProtocol.ErrorCode.ResourceNotFound,
      message: "Resource not found",
    } satisfies Partial<TalosApiError>);
  });

  it("rejects unregistered protocol error codes", async () => {
    const transport = createTransportHarness();
    const request = transport.instance.request("session/get", {
      sessionId: "session-1",
    });

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      error: {
        code: -32000,
        message: "unregistered",
      },
    }));

    await expect(request).rejects.toThrow("Invalid app-server protocol error.");
  });
});

describe("JsonRpcStdioTransport notifications", () => {
  it("emits typed message queue change notifications", () => {
    const transport = createTransportHarness();
    const changes: unknown[] = [];
    transport.instance.onMessageQueueChanged((change) => changes.push(change));

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      method: "messageQueue/changed",
      params: { sessionId: "session-1", revision: 5 },
    }));

    expect(changes).toEqual([{ sessionId: "session-1", revision: 5 }]);
  });

  it("emits typed subscription termination notifications", () => {
    const transport = createTransportHarness();
    const terminated: unknown[] = [];
    transport.instance.onSubscriptionTerminated((notification) => terminated.push(notification));

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      method: "subscription/terminated",
      params: {
        subscriptionId: "sub-1",
        sessionId: "session-1",
        reason: "slow_consumer",
        message: "subscription lagged behind retained live events",
      },
    }));

    expect(terminated).toEqual([
      {
        subscriptionId: "sub-1",
        sessionId: "session-1",
        reason: "slow_consumer",
        message: "subscription lagged behind retained live events",
      },
    ]);
  });

  it("emits transport errors without using EventEmitter error semantics", () => {
    const transport = createTransportHarness();
    const errors: Error[] = [];
    transport.instance.onError((error) => errors.push(error));

    expect(() => transport.handleLine("{")).not.toThrow();
    expect(errors).toMatchObject([{ code: AppProtocol.ErrorCode.InternalError, message: "The backend returned invalid JSON.", cause: expect.any(SyntaxError) }]);
  });

  it.each(["null", "SECRET_REMOTE_BODY", '{"method":42}'])("rejects pending calls for an invalid frame: %s", async (frame) => {
    const transport = createTransportHarness();
    const operation = transport.instance.request("session/list", {});
    const assertion = expect(operation).rejects.toBeInstanceOf(Error);
    transport.handleLine(frame);
    await assertion;
  });

  it.each([null, { error: { code: "internalError", message: "Invalid response." }, retryable: false }])("reports malformed terminal data instead of dropping it", (data) => {
    const transport = createTransportHarness();
    const error = vi.fn();
    const event = vi.fn();
    const terminated = vi.fn();
    transport.instance.onError(error);
    transport.instance.onEvent(event);
    transport.instance.onSubscriptionTerminated(terminated);
    transport.handleLine(JSON.stringify({ jsonrpc: "2.0", method: "timeline/turn/failed", params: {
      subscriptionId: "sub-1", event: { eventId: "event-1", eventSeq: 1, persistedAt: 1, event: { type: "agent", data: {
        identity: { sessionId: "session-1", turnId: "turn-1" }, sequence: 1,
        payload: { type: "turnFailed", data },
      } } },
    } }));
    expect(error).toHaveBeenCalledOnce();
    expect(event).not.toHaveBeenCalled();
    expect(terminated.mock.calls).toEqual([[{
      subscriptionId: "sub-1", sessionId: "session-1", reason: "interrupted",
      message: "The app-server returned an invalid timeline event.",
    }]]);
  });
});

describe("filterAppServerEnvironment", () => {
  it("keeps talos variables and minimal process variables", () => {
    expect(
      filterAppServerEnvironment({
        TALOS_HOME: "/tmp/talos",
        TALOS_LLM_ANTHROPIC_API_KEY: "sk-test",
        HOME: "/Users/example",
        PATH: "/usr/bin",
        ANTHROPIC_API_KEY: "ignored",
        npm_config_user_agent: "ignored",
      }),
    ).toEqual({
      TALOS_HOME: "/tmp/talos",
      TALOS_LLM_ANTHROPIC_API_KEY: "sk-test",
      HOME: "/Users/example",
      PATH: "/usr/bin",
    });
  });
});

function createTransportHarness() {
  const instance = new JsonRpcStdioTransport();
  const writes: string[] = [];
  const unsafe = instance as unknown as {
    child?: { stdin: { write(line: string): void } };
    handleLine(line: string): void;
  };
  unsafe.child = {
    stdin: {
      write(line: string) {
        writes.push(line.trim());
      },
    },
  };
  return {
    instance,
    writes,
    handleLine: (line: string) => unsafe.handleLine(line),
  };
}
