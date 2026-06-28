import { describe, expect, it } from "vitest";

import { AppServerProtocolError, JsonRpcStdioTransport, filterAppServerEnvironment } from "./stdio";

describe("AppServerProtocolError", () => {
  it("preserves protocol error fields", () => {
    const error = new AppServerProtocolError({
      code: -32002,
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
      expect(request.method).toBe("client.user_input.request");
      return { text: "confirmed" };
    });

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      id: 7,
      method: "client.user_input.request",
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
      method: "client.unknown",
      params: null,
    }));
    await Promise.resolve();

    expect(transport.writes.map((line) => JSON.parse(line))).toEqual([
      {
        jsonrpc: "2.0",
        id: 9,
        error: {
          code: -32601,
          message: "unsupported server request: client.unknown",
        },
      },
    ]);
  });
});

describe("JsonRpcStdioTransport notifications", () => {
  it("emits typed subscription termination notifications", () => {
    const transport = createTransportHarness();
    const terminated: unknown[] = [];
    transport.instance.onSubscriptionTerminated((envelope) => terminated.push(envelope));

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      method: "subscription.terminated",
      params: {
        subscription_id: "sub-1",
        session_id: "session-1",
        reason: "slow_consumer",
        message: "subscription lagged behind retained live events",
      },
    }));

    expect(terminated).toEqual([
      {
        subscription_id: "sub-1",
        session_id: "session-1",
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
    expect(errors[0]?.message).toContain("Expected property name");
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
