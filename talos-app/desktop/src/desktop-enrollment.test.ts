import { describe, expect, it, vi } from "vitest";
import { AppProtocol } from "@talos/sdk-core";
import { DesktopAppServerController } from "./app-server-controller";
import { FakeBridge } from "./app-server-controller.test-support";

function hostBridge(mode: "cloud" | "local", savedLogin: boolean, authEnabled = true): FakeBridge {
  const bridge = new FakeBridge();
  const connect = bridge.connect.bind(bridge);
  vi.spyOn(bridge, "connect").mockImplementation(async () => {
    const response = await connect();
    return { ...response, server_info: { ...response.server_info, auth_enabled: authEnabled } };
  });
  vi.spyOn(bridge, "invoke").mockImplementation(<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> => {
    bridge.requests.push({ method, params });
    const responses: Partial<Record<AppProtocol.ClientMethod, unknown>> = {
      "desktop/connection/read": { connectionMode: mode, controlPlaneEndpoint: "https://agent.talos.love" },
      "desktop/connection/set": { ...(params as AppProtocol.DesktopConnectionSetParams), controlPlaneEndpoint: "https://agent.talos.love" },
      "auth/session/read": { session: savedLogin ? { access_token: "USER_ACCESS_TOKEN" } : null },
      "auth/login/status": { status: "authenticated", access_token: "USER_ACCESS_TOKEN" },
      "desktop/workerEnroll": { nodeId: "stable-uuid", controlPlaneEndpoint: "https://agent.talos.love" },
    };
    return Promise.resolve(responses[method] as T);
  });
  return bridge;
}

describe("Desktop host enrollment", () => {
  it("enrolls after login and resumes the saved identity after app-server restarts", async () => {
    vi.useFakeTimers();
    const initial = hostBridge("cloud", false);
    const restored = hostBridge("cloud", true);
    const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: vi.fn().mockReturnValueOnce(initial).mockReturnValue(restored) });
    try {
      await controller.connect();
      expect(initial.requests.map((request) => request.method)).toEqual(["desktop/connection/read", "auth/session/read"]);
      await controller.invoke("auth/login/status", { login_id: "login-1" });
      expect(initial.requests.at(-1)).toEqual({ method: "desktop/workerEnroll", params: { nodeId: null } });
      initial.emitExit();
      await vi.advanceTimersByTimeAsync(300);
      expect(restored.requests.map((request) => request.method)).toEqual(["desktop/connection/read", "auth/session/read", "desktop/workerEnroll"]);
      expect(restored.requests.some((request) => request.method === "computer/create")).toBe(false);
    } finally {
      await controller.close();
      vi.useRealTimers();
    }
  });

  it.each([false, true])("persists cloud mode for authenticated Desktop with stored local mode (saved login: %s)", async (savedLogin) => {
    const bridge = hostBridge("local", savedLogin);
    const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: () => bridge });
    try {
      await controller.connect();
      expect(bridge.requests).toEqual([
        { method: "desktop/connection/read", params: null },
        { method: "desktop/connection/set", params: { connectionMode: "cloud" } },
        { method: "auth/session/read", params: {} },
        ...(savedLogin ? [{ method: "desktop/workerEnroll", params: { nodeId: null } }] : []),
      ]);
      if (!savedLogin) {
        await controller.invoke("auth/login/status", { login_id: "login-1" });
        expect(bridge.requests.at(-1)).toEqual({ method: "desktop/workerEnroll", params: { nodeId: null } });
      }
    } finally { await controller.close(); }
  });

  it("reports a failed cloud configuration write before becoming ready or enrolling", async () => {
    const bridge = hostBridge("local", true);
    const invoke = bridge.invoke.bind(bridge);
    vi.spyOn(bridge, "invoke").mockImplementation(<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> => {
      if (method === "desktop/connection/set") return Promise.reject(new Error("Configuration write failed"));
      return invoke<T>(method, params);
    });
    const states = vi.fn();
    const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: () => bridge, onRuntimeState: states });
    try {
      await expect(controller.connect()).rejects.toThrow("Configuration write failed");
      expect(states.mock.calls).toEqual([[{ kind: "starting", message: null }]]);
      expect(bridge.requests.map((request) => request.method)).toEqual(["desktop/connection/read"]);
    } finally { await controller.close(); }
  });

  it("preserves explicit auth-disabled development local mode without reacting to stale exits", async () => {
    const cloud = hostBridge("cloud", false);
    const local = hostBridge("local", true, false);
    const factory = vi.fn().mockReturnValueOnce(cloud).mockReturnValue(local);
    const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: factory });
    try {
      await controller.connect();
      await controller.invoke("desktop/connection/set", { connectionMode: "local" });
      cloud.emitExit();
      expect(cloud.closed).toBe(true);
      expect(factory).toHaveBeenCalledTimes(2);
      expect(local.requests).toEqual([{ method: "desktop/connection/read", params: null }]);
    } finally { await controller.close(); }
  });

  it("surfaces failed cloud enrollment without switching to a local host or discarding login", async () => {
    const bridge = hostBridge("cloud", true);
    const factory = vi.fn().mockReturnValue(bridge);
    const invoke = bridge.invoke.bind(bridge);
    vi.spyOn(bridge, "invoke").mockImplementation(<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> => {
      if (method === "desktop/workerEnroll") {
        return Promise.reject(new Error("Enrollment unavailable"));
      }
      return invoke<T>(method, params);
    });
    const states = vi.fn();
    const report = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: factory, onRuntimeState: states });
    try {
      await controller.connect();
      expect(states.mock.calls.at(-1)).toEqual([{ kind: "error", message: "Worker registration failed. Recover or reconnect this computer to retry.", error: { code: "internalError", message: "Worker registration failed; technical details are available in the desktop log.", localization: null } }]);
      const login = await controller.invoke<AppProtocol.CloudLoginStatusResponse>("auth/login/status", { login_id: "login-1" });
      expect(login.status).toBe("authenticated");
      expect(factory).toHaveBeenCalledOnce();
      expect(bridge.requests.some((request) => request.method === "auth/session/clear" || request.method === "desktop/connection/set")).toBe(false);
    } finally { await controller.close(); report.mockRestore(); }
  });
});

it("delivers an asynchronous Worker rejection without losing the authenticated host", async () => {
  const bridge = hostBridge("cloud", true);
  const states = vi.fn();
  const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: () => bridge, onRuntimeState: states });
  try {
    await controller.connect();
    const error: AppProtocol.ApiError = { code: "permissionDenied", message: "Worker credential was revoked.", localization: null };
    bridge.emitNotification({ jsonrpc: "2.0", method: "desktop/worker/failed", params: error });
    expect(states.mock.calls.at(-1)).toEqual([{ kind: "error", message: null, error }]);
    expect(bridge.closed).toBe(false);
    expect(bridge.requests.some((request) => request.method === "auth/session/clear")).toBe(false);
    await controller.close();
    const count = states.mock.calls.length;
    bridge.emitNotification({ jsonrpc: "2.0", method: "desktop/worker/failed", params: error });
    expect(states.mock.calls).toHaveLength(count);
  } finally { await controller.close(); }
});
