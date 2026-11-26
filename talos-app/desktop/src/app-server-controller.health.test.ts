import { describe, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-core";

import { FakeBridge } from "./app-server-controller.test-support";
import { DesktopAppServerController } from "./app-server-controller";

describe("DesktopAppServerController runtime health", () => {
  it("forwards real-time active and queue notices without treating Worker registration as runtime health", async () => {
    const bridge = new FakeBridge();
    const onActiveTurnChanged = vi.fn();
    const onMessageQueueChanged = vi.fn();
    const onAutomationChanged = vi.fn();
    const onRuntimeState = vi.fn();
    const controller = new DesktopAppServerController({ bridgeFactory: () => bridge, onActiveTurnChanged, onMessageQueueChanged, onAutomationChanged, onRuntimeState });
    await controller.connect();
    await controller.invoke("event/subscribe", { sessionId: "shared", turnId: null, sinceEventId: null, replayLimit: null, closeWhenIdle: false, closeOnTerminal: false });
    const active = { sessionId: "shared", state: { activeTurn: null, latestEventId: null } };
    bridge.emitNotification({ jsonrpc: "2.0", method: "session/activeTurnChanged", params: active });
    bridge.emitNotification({ jsonrpc: "2.0", method: "messageQueue/changed", params: { sessionId: "shared", revision: 2 } });
    bridge.emitNotification({ jsonrpc: "2.0", method: "desktop/worker/failed", params: { code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Computer unavailable", localization: null } });
    bridge.emitNotification({ jsonrpc: "2.0", method: "desktop/worker/ready", params: null });
    bridge.emitNotification({ jsonrpc: "2.0", method: "automation/changed", params: { sessionId: "shared" } });
    bridge.emitNotification({ jsonrpc: "2.0", method: "automation/changed", params: { sessionId: "other-session" } });
    expect(onAutomationChanged.mock.calls).toEqual([[{ sessionId: "shared" }]]);
    expect(onActiveTurnChanged).toHaveBeenCalledWith(active);
    expect(onMessageQueueChanged).toHaveBeenCalledWith({ sessionId: "shared", revision: 2 });
    expect(onRuntimeState.mock.calls).toEqual([
      [{ kind: "starting", message: null }],
      [{ kind: "ready", message: null }],
    ]);
    await controller.close();
  });

  it("replays the current runtime health when a renderer reconnects to the initialized bridge", async () => {
    const bridge = new FakeBridge();
    const connect = vi.spyOn(bridge, "connect");
    const states = vi.fn();
    const controller = new DesktopAppServerController({ bridgeFactory: () => bridge, onRuntimeState: states });
    try {
      const initialized = await controller.connect();
      states.mockClear();
      expect(await controller.connect()).toBe(initialized);
      expect(connect).toHaveBeenCalledOnce();
      expect(states.mock.calls).toEqual([[{ kind: "ready", message: null }]]);
    } finally {
      await controller.close();
    }
  });

  it("does not let Worker recovery conceal a real local transport failure", async () => {
    const bridge = new FakeBridge();
    const states = vi.fn();
    const report = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const controller = new DesktopAppServerController({ bridgeFactory: () => bridge, onRuntimeState: states });
    try {
      await controller.connect();
      const failure = { code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Transport unavailable", localization: null };
      bridge.emitError(new TalosApiError(failure));
      expect(states.mock.calls.at(-1)?.[0].kind).toBe("error");
      const count = states.mock.calls.length;
      bridge.emitNotification({ jsonrpc: "2.0", method: "desktop/worker/ready", params: null });
      await controller.invoke("desktop/workerEnroll", {});
      expect(states.mock.calls).toHaveLength(count);
      const actualError = states.mock.calls.at(-1)?.[0];
      expect(actualError).toMatchObject({ kind: "error", error: failure });
      await controller.connect();
      expect(states.mock.calls).toHaveLength(count + 1);
      expect(states.mock.calls.at(-1)?.[0]).toEqual(actualError);
    } finally {
      await controller.close();
      report.mockRestore();
    }
  });
});
