import { expect, it, vi } from "vitest";
import { createTrajectoryClient, type TrajectoryTransport } from "../trajectory";
import { TalosClient } from "../client";
import { AppServerClient } from "../app-server";
const identity = { sessionId: "s", turnId: "t" };
const ref = { payloadId: "p", complete: true };
function transport(readPayload = vi.fn()): TrajectoryTransport { return { capabilities: vi.fn(), readTurn: vi.fn(), readRequest: vi.fn(), readTool: vi.fn(), readPayload }; }
it("uses identical typed requests for independent HTTP and RPC operations", async () => {
    const request = vi.fn().mockResolvedValue({});
    const http = new TalosClient({ request }, { stream: vi.fn() }, "https://talos.example");
    const rpc = new AppServerClient({ request, onEvent: vi.fn() });
    const params = { identity, cursor: null, limit: 32 };
    await http.trajectory.readTurn(params);
    await rpc.trajectory.readTurn(params);
    expect(request).toHaveBeenNthCalledWith(1, "https://talos.example/v1/trajectory/turn/read", { method: "POST", body: JSON.stringify(params), signal: undefined });
    expect(request).toHaveBeenNthCalledWith(2, "trajectory/readTurn", params);
});
it("assembles immutable UTF-8 pages and preserves exact raw parameter strings", async () => {
    const raw = '{ "text": "你好" }\n';
    const value = { kind: "response", parts: [{ type: "toolCall", call: { callId: "c", name: "Read", arguments: { text: "你好" }, argumentsText: raw } }] };
    const body = JSON.stringify(value), split = body.indexOf("你好");
    const read = vi.fn().mockResolvedValueOnce({ payloadId: "p", encoding: "utf8Json", data: body.slice(0, split), nextCursor: "next" })
        .mockResolvedValueOnce({ payloadId: "p", encoding: "utf8Json", data: body.slice(split), nextCursor: null });
    expect(await createTrajectoryClient(transport(read)).payload(identity, ref)).toEqual(value);
    expect(read.mock.calls[1][0].cursor).toBe("next");
});
it("does not read absent payloads and distinguishes known empty snapshots", async () => {
    const read = vi.fn();
    expect(await createTrajectoryClient(transport(read)).payload(identity, null)).toBeNull();
    expect(read).not.toHaveBeenCalled();
    const empty = { kind: "tools", definitions: [] };
    read.mockResolvedValue({ payloadId: "p", encoding: "utf8Json", data: JSON.stringify(empty), nextCursor: null });
    expect(await createTrajectoryClient(transport(read)).payload(identity, ref)).toEqual(empty);
});
it("rejects repeated cursors, wrong payload identity and oversized pages", async () => {
    for (const page of [
        { payloadId: "p", encoding: "utf8Json", data: "{", nextCursor: "loop" },
        { payloadId: "other", encoding: "utf8Json", data: "{}", nextCursor: null },
        { payloadId: "p", encoding: "utf8Json", data: "x".repeat(256 * 1024 + 1), nextCursor: null },
    ])
        await expect(createTrajectoryClient(transport(vi.fn().mockResolvedValue(page))).payload(identity, ref)).rejects.toThrow();
});
it("preserves real read errors instead of marking a recorded snapshot unsupported", async () => {
    const failure = new Error("read failed");
    await expect(createTrajectoryClient(transport(vi.fn().mockRejectedValue(failure))).payload(identity, ref)).rejects.toBe(failure);
});
it("checks cancellation before issuing the next page", async () => {
    const control = new AbortController();
    control.abort();
    const read = vi.fn();
    await expect(createTrajectoryClient(transport(read)).payload(identity, ref, control.signal)).rejects.toThrow();
    expect(read).not.toHaveBeenCalled();
});
it("recognizes only an absent capability method as an old unsupported host", async () => {
    const old = transport();
    old.capabilities = vi.fn().mockRejectedValue(Object.assign(new Error("method absent"), { code: -32601 }));
    expect(await createTrajectoryClient(old).capabilities()).toEqual({ supported: false });
    const denied = new Error("denied");
    old.capabilities = vi.fn().mockRejectedValue(denied);
    await expect(createTrajectoryClient(old).capabilities()).rejects.toBe(denied);
});
