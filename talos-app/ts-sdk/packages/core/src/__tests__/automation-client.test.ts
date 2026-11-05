import { describe, expect, it, vi } from "vitest";
import { AppServerClient } from "../app-server";
import { TalosClient } from "../client";
import { TalosApiError } from "../adapters";
import { ErrorCode } from "../generated/app-protocol";

const session = { sessionId: "s1" };
const pause = { ...session, action: { target: "goal" as const, action: { type: "pause" as const, goalId: "g1" } } };
const list = { sessionId: null, scope: "unfinished" as const, cursor: null, limit: 20 };
const computer = { computerId: "c1" };
const control = { ...computer, executionInstanceId: "i1", jobId: "profileUsageRetention" as const, action: "pause" as const };

describe("automation public clients", () => {
  it("maps both transports to the same typed operations without a local command parser", async () => {
    const request = vi.fn().mockResolvedValue({});
    const http = new TalosClient({ request }, { stream: vi.fn() }, "http://localhost");
    const rpc = new AppServerClient({ request, onEvent: () => () => undefined });
    const expectedBodies = [session, pause, list, {}, { text: "/loop 5m check CI" }, computer, control];
    for (const client of [http, rpc]) {
      await client.automation.read(session);
      await client.automation.execute(pause);
      await client.automation.list(list);
      await client.automation.commandCatalog({});
      await client.automation.resolveCommand({ text: "/loop 5m check CI" });
      await client.builtinJobs.list(computer);
      await client.builtinJobs.control(control);
    }
    expect(request.mock.calls.slice(0, 7).map(([url, options]) => [url, JSON.parse(String(options.body))])).toEqual(
      ["automation/read", "automation/execute", "automation/list", "automation/command-catalog", "automation/resolve-command", "builtin-job/list", "builtin-job/control"]
        .map((path, index) => [`http://localhost/v1/${path}`, expectedBodies[index]]),
    );
    expect(request.mock.calls.slice(7)).toEqual(
      ["automation/read", "automation/execute", "automation/list", "automation/commandCatalog", "automation/resolveCommand", "builtinJob/list", "builtinJob/control"]
        .map((method, index) => [method, expectedBodies[index]]),
    );
  });

  it("preserves a typed authorization failure instead of returning an empty list", async () => {
    const error = new TalosApiError({ code: ErrorCode.PermissionDenied, message: "Denied", localization: null });
    const request = vi.fn().mockRejectedValue(error);
    const client = new TalosClient({ request }, { stream: vi.fn() }, "http://localhost");
    await expect(client.builtinJobs.list(computer)).rejects.toBe(error);
  });

  it("dispatches automation invalidation separately from timeline execution", async () => {
    const onAutomationChanged = vi.fn();
    const onEvent = vi.fn();
    const stream = vi.fn().mockImplementation(async (_url, _options, onMessage) => {
      onMessage(JSON.stringify(session), "automation/changed");
    });
    const client = new TalosClient({ request: vi.fn() }, { stream }, "http://localhost");
    await client.turns.events("s1", undefined, onEvent, undefined, { onAutomationChanged });
    expect(onAutomationChanged).toHaveBeenCalledWith(session);
    expect(onEvent).not.toHaveBeenCalled();
  });
});
