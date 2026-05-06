import { describe, expect, it } from "vitest";

import { AppServerProtocolError, JsonRpcStdioTransport } from "./stdio";

describe("AppServerProtocolError", () => {
  it("preserves protocol error fields", () => {
    const error = new AppServerProtocolError({
      code: -32002,
      message: "missing session",
      data: { type: "resource", kind: "not_found", resource: "session missing" },
    });

    expect(error.name).toBe("AppServerProtocolError");
    expect(error.message).toBe("missing session");
    expect(error.code).toBe(-32002);
    expect(error.data).toEqual({
      type: "resource",
      kind: "not_found",
      resource: "session missing",
    });
  });
});

describe("JsonRpcStdioTransport server requests", () => {
  it("responds to server requests with registered handler output", async () => {
    const transport = createTransportHarness();
    transport.instance.onServerRequest((request) => {
      expect(request.method).toBe("client.auth.refresh");
      return { token: "next-token" };
    });

    transport.handleLine(JSON.stringify({
      jsonrpc: "2.0",
      id: 7,
      method: "client.auth.refresh",
      params: { reason: "expired" },
    }));
    await Promise.resolve();

    expect(transport.writes.map((line) => JSON.parse(line))).toEqual([
      {
        jsonrpc: "2.0",
        id: 7,
        result: { token: "next-token" },
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
          data: null,
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
