import { describe, expect, it, vi } from "vitest";
import { AppProtocol } from "@talos/sdk-core";
import { DesktopAppServerController } from "./app-server-controller";
import { FakeBridge } from "./app-server-controller.test-support";

function hostBridge(savedLogin: boolean, authEnabled = true): FakeBridge {
  const bridge = new FakeBridge();
  const connect = bridge.connect.bind(bridge);
  vi.spyOn(bridge, "connect").mockImplementation(async () => {
    const response = await connect();
    return { ...response, server_info: { ...response.server_info, auth_enabled: authEnabled } };
  });
  vi.spyOn(bridge, "invoke").mockImplementation(<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> => {
    bridge.requests.push({ method, params });
    const responses: Partial<Record<AppProtocol.ClientMethod, unknown>> = {
      "auth/session/read": { session: savedLogin ? { access_token: "USER_ACCESS_TOKEN", user: { user_id: "owner" } } : null },
      "auth/login/status": { status: "authenticated", access_token: "USER_ACCESS_TOKEN", user: { user_id: "owner" } },
      "sync/status": { localComputerId: "stable-uuid" },
    };
    return Promise.resolve(responses[method] as T);
  });
  return bridge;
}

describe("Desktop account lifecycle", () => {
  it("leaves enrollment to the native lifecycle after login and restart", async () => {
    vi.useFakeTimers();
    const initial = hostBridge(false);
    const restored = hostBridge(true);
    const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: vi.fn().mockReturnValueOnce(initial).mockReturnValue(restored) });
    try {
      await controller.connect();
      expect(initial.requests.map((request) => request.method)).toEqual(["auth/session/read"]);
      await controller.invoke("auth/login/status", { login_id: "login-1" });
      expect(initial.requests.at(-1)?.method).toBe("auth/login/status");
      expect(initial.requests.some((request) => request.method === "sync/status")).toBe(false);
      initial.emitExit();
      await vi.advanceTimersByTimeAsync(300);
      expect(restored.requests.map((request) => request.method)).toEqual(["auth/session/read"]);
      expect(restored.requests.some((request) => request.method === "computer/create")).toBe(false);
    } finally {
      await controller.close();
      vi.useRealTimers();
    }
  });

  it("keeps anonymous development on its existing local Worker", async () => {
    const bridge = hostBridge(true, false);
    const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: () => bridge });
    try {
      await controller.connect();
      expect(bridge.requests).toEqual([]);
    } finally { await controller.close(); }
  });

  it("keeps snapshot query errors separate from login and native enrollment", async () => {
    const bridge = hostBridge(true);
    const factory = vi.fn().mockReturnValue(bridge);
    const invoke = bridge.invoke.bind(bridge);
    vi.spyOn(bridge, "invoke").mockImplementation(<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> => {
      if (method === "sync/status") {
        return Promise.reject(new Error("Enrollment unavailable"));
      }
      return invoke<T>(method, params);
    });
    const states = vi.fn();
    const report = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: factory, onRuntimeState: states });
    try {
      await controller.connect();
      await expect(controller.invoke("sync/status", null)).rejects.toThrow("Enrollment unavailable");
      expect(states.mock.calls.at(-1)?.[0].kind).toBe("ready");
      const login = await controller.invoke<AppProtocol.CloudLoginStatusResponse>("auth/login/status", { login_id: "login-1" });
      expect(login.status).toBe("authenticated");
      expect(factory).toHaveBeenCalledOnce();
      expect(bridge.requests.some((request) => request.method === "auth/session/clear")).toBe(false);
    } finally { await controller.close(); report.mockRestore(); }
  });
});

it("delivers an asynchronous Worker rejection without losing the authenticated host", async () => {
  const bridge = hostBridge(true);
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

it("serves local requests without using a pending status query for enrollment", async () => {
  const bridge = hostBridge(true);
  const invoke = bridge.invoke.bind(bridge);
  let release!: () => void;
  const registration = new Promise<void>((resolve) => { release = resolve; });
  vi.spyOn(bridge, "invoke").mockImplementation(<T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> => {
    if (method === "sync/status") {
      bridge.requests.push({ method, params });
      return registration.then(() => ({ localComputerId: "stable-uuid" }) as T);
    }
    return invoke<T>(method, params);
  });
  const states = vi.fn();
  const controller = new DesktopAppServerController({ desktopHost: true, bridgeFactory: () => bridge, onRuntimeState: states });
  try {
    let connected = false;
    const connection = controller.connect().then(() => { connected = true; });
    await vi.waitFor(() => expect(connected).toBe(true));
    await connection;
    let restored = false;
    const cache = controller.invoke("auth/session/read", {}).then(() => { restored = true; });
    await vi.waitFor(() => expect(restored).toBe(true));
    await cache;
    const status = controller.invoke("sync/status", null);
    await controller.invoke("session/history", { sessionId: "retained", cursor: null, limit: null });
    expect(bridge.requests.filter((request) => request.method === "sync/status")).toHaveLength(1);
    expect(bridge.requests.at(-1)?.method).toBe("session/history");
    await controller.close();
    const count = states.mock.calls.length;
    release();
    await status;
    await Promise.resolve();
    await Promise.resolve();
    expect(states.mock.calls).toHaveLength(count);
  } finally {
    release();
    await controller.close();
  }
});
