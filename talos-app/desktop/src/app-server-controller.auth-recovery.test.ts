import { AppProtocol, TalosApiError } from "@talos/sdk-core";
import { describe, expect, it, vi } from "vitest";

import { DesktopAppServerController, type AppServerBridge } from "./app-server-controller";
import { FakeBridge } from "./app-server-controller.test-support";

const credentials = (token: string) => ({ actorCredentials: { credential: { type: "existing_session_token" as const, token } } });
const subscribe = (sessionId: string) => ({ sessionId, turnId: null, sinceEventId: null, replayLimit: 50, closeWhenIdle: false, closeOnTerminal: false });
const unauthorized = () => new TalosApiError({ code: "unauthenticated", message: "Sign in required.", localization: null });

function authenticatedBridge(userId = "alice", token = "current-token", reject = false) {
  const bridge = new FakeBridge();
  const connect = bridge.connect.bind(bridge);
  vi.spyOn(bridge, "connect").mockImplementation(async () => {
    const response = await connect();
    return { ...response, server_info: { ...response.server_info, auth_enabled: true } };
  });
  const invoke = bridge.invoke.bind(bridge);
  const requests = vi.spyOn(bridge as AppServerBridge, "invoke").mockImplementation(async <T>(
    method: AppProtocol.ClientMethod, params: unknown, options?: { actorCredentials?: AppProtocol.ActorCredentials | null },
  ): Promise<T> => {
    if (method === "auth/session/read") return { session: { access_token: token, user: { user_id: userId } } } as T;
    if (method === "user/current" || method === "event/subscribe" || method === "event/unsubscribe") {
      if (reject || options?.actorCredentials?.credential.token !== token) throw unauthorized();
      if (method === "user/current") return { user: { user_id: userId } } as T;
    }
    return invoke<T>(method, params);
  });
  return { bridge, requests };
}

function event(sessionId: string, eventId: string): AppProtocol.TimelineEvent {
  return { eventId, eventSeq: 1, persistedAt: 1, event: { type: "agent", data: {
    identity: { sessionId, turnId: "turn" }, sequence: 0, occurredAt: 1, payload: { type: "turnStarted", data: {} },
  } } };
}

describe("authenticated Desktop restart recovery", () => {
  it("uses the current same-account token for multiple cursors and keeps original unsubscribe handles across two restarts", async () => {
    vi.useFakeTimers();
    const initial = authenticatedBridge("alice", "old-token");
    const invoke = initial.requests.getMockImplementation()!;
    initial.requests.mockImplementation(async <T>(method: AppProtocol.ClientMethod, params: unknown, options?: { actorCredentials?: AppProtocol.ActorCredentials | null }): Promise<T> => {
      const result = await invoke(method, params, options);
      // A fast backend can emit replay lines before the subscribe response's promise resumes.
      if (method === "event/subscribe" && (params as AppProtocol.SubscribeTimelineRequest).sessionId === "first") initial.bridge.emitEvent(event("first", "event-early"));
      return result as T;
    });
    const replacement = authenticatedBridge();
    const third = authenticatedBridge();
    const events = vi.fn();
    const states = vi.fn();
    const controller = new DesktopAppServerController({
      restartDelayMs: 10, onEvent: events, onRuntimeState: states,
      bridgeFactory: vi.fn().mockReturnValueOnce(initial.bridge).mockReturnValueOnce(replacement.bridge).mockReturnValue(third.bridge),
    });
    try {
      await controller.connect();
      const first = await controller.invoke<AppProtocol.SubscribeTimelineResponse>("event/subscribe", subscribe("first"), credentials("old-token"));
      await controller.invoke("event/subscribe", subscribe("second"), credentials("old-token"));
      const removed = await controller.invoke<AppProtocol.SubscribeTimelineResponse>("event/subscribe", subscribe("removed"), credentials("old-token"));
      await controller.invoke("event/unsubscribe", { subscriptionId: removed.subscriptionId }, credentials("old-token"));
      initial.bridge.emitEvent(event("first", "event-7"));
      initial.bridge.emitEvent(event("second", "event-9"));
      initial.bridge.emitExit();
      await vi.advanceTimersByTimeAsync(10);
      const replays = replacement.requests.mock.calls.filter(([method]) => method === "event/subscribe");
      expect(replays).toEqual([
        ["event/subscribe", { ...subscribe("first"), sinceEventId: "event-7" }, credentials("current-token")],
        ["event/subscribe", { ...subscribe("second"), sinceEventId: "event-9" }, credentials("current-token")],
      ]);
      initial.bridge.emitEvent(event("first", "stale-bridge-event"));
      replacement.bridge.emitEvent(event("first", "event-10"));
      await controller.invoke("event/unsubscribe", { subscriptionId: first.subscriptionId }, credentials("current-token"));
      replacement.bridge.emitEvent(event("first", "late-unsubscribed-event"));
      replacement.bridge.emitExit();
      await vi.advanceTimersByTimeAsync(10);
      expect(third.requests.mock.calls.filter(([method]) => method === "event/subscribe")).toEqual([
        ["event/subscribe", { ...subscribe("second"), sinceEventId: "event-9" }, credentials("current-token")],
      ]);
      expect(events.mock.calls.map(([value]) => value.eventId)).toEqual(["event-early", "event-7", "event-9", "event-10"]);
      expect(states.mock.calls.some(([state]) => state.kind === "error")).toBe(false);
    } finally { await controller.close(); vi.useRealTimers(); }
  });

  it.each(["other-account", "rejected-token", "expired-cache"])("terminates instead of replaying with %s", async (failure) => {
    vi.useFakeTimers();
    const initial = authenticatedBridge();
    const replacement = authenticatedBridge(failure === "other-account" ? "bob" : "alice", "current-token", failure === "rejected-token");
    if (failure === "expired-cache") {
      const invoke = replacement.requests.getMockImplementation()!;
      replacement.requests.mockImplementation(async <T>(method: AppProtocol.ClientMethod, params: unknown, options?: { actorCredentials?: AppProtocol.ActorCredentials | null }): Promise<T> => {
        if (method === "auth/session/read") return { session: null } as T;
        return invoke(method, params, options) as Promise<T>;
      });
    }
    const terminated = vi.fn();
    const states = vi.fn();
    const report = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const controller = new DesktopAppServerController({ restartDelayMs: 10, onRuntimeState: states, onSubscriptionTerminated: terminated,
      bridgeFactory: vi.fn().mockReturnValueOnce(initial.bridge).mockReturnValue(replacement.bridge) });
    try {
      await controller.connect();
      await controller.invoke("event/subscribe", subscribe("private"), credentials("current-token"));
      initial.bridge.emitExit();
      await vi.advanceTimersByTimeAsync(10);
      expect(terminated).toHaveBeenCalledOnce();
      expect(states.mock.calls.at(-1)?.[0]).toMatchObject({ kind: "error", error: { code: "unauthenticated" } });
      if (failure !== "rejected-token") expect(replacement.requests.mock.calls.some(([method]) => method === "event/subscribe")).toBe(false);
    } finally { await controller.close(); report.mockRestore(); vi.useRealTimers(); }
  });

  it.each(["auth/logout", "auth/session/clear"] as const)("forgets subscriptions on %s before restart", async (method) => {
    vi.useFakeTimers();
    const initial = authenticatedBridge();
    const replacement = authenticatedBridge("bob");
    const events = vi.fn();
    const controller = new DesktopAppServerController({ restartDelayMs: 10, onEvent: events,
      bridgeFactory: vi.fn().mockReturnValueOnce(initial.bridge).mockReturnValue(replacement.bridge) });
    try {
      await controller.connect();
      await controller.invoke("event/subscribe", subscribe("private"), credentials("current-token"));
      await controller.invoke(method, {}, credentials("current-token"));
      initial.bridge.emitEvent(event("private", "late-logout-event"));
      initial.bridge.emitExit();
      await vi.advanceTimersByTimeAsync(10);
      expect(replacement.requests.mock.calls.some(([request]) => request === "event/subscribe")).toBe(false);
      expect(events).not.toHaveBeenCalled();
    } finally { await controller.close(); vi.useRealTimers(); }
  });

  it("does not start a replacement backend after shutdown during the restart delay", async () => {
    vi.useFakeTimers();
    const initial = authenticatedBridge();
    const factory = vi.fn().mockReturnValue(initial.bridge);
    const controller = new DesktopAppServerController({ restartDelayMs: 10, bridgeFactory: factory });
    try {
      await controller.connect();
      initial.bridge.emitExit();
      await controller.close();
      await vi.advanceTimersByTimeAsync(10);
      expect(factory).toHaveBeenCalledOnce();
    } finally { vi.useRealTimers(); }
  });

  it("discards an in-flight subscription when a new account finishes login", async () => {
    vi.useFakeTimers();
    const initial = authenticatedBridge();
    const replacement = authenticatedBridge("bob");
    const events = vi.fn();
    let release!: () => void;
    let entered!: () => void;
    const blocked = new Promise<void>((resolve) => { release = resolve; });
    const started = new Promise<void>((resolve) => { entered = resolve; });
    const invoke = initial.requests.getMockImplementation()!;
    initial.requests.mockImplementation(async <T>(method: AppProtocol.ClientMethod, params: unknown, options?: { actorCredentials?: AppProtocol.ActorCredentials | null }): Promise<T> => {
      if (method === "auth/login/status") return { status: "authenticated", user: { user_id: "bob" } } as T;
      if (method === "event/subscribe") { entered(); await blocked; }
      return invoke(method, params, options) as Promise<T>;
    });
    const controller = new DesktopAppServerController({ restartDelayMs: 10, onEvent: events,
      bridgeFactory: vi.fn().mockReturnValueOnce(initial.bridge).mockReturnValue(replacement.bridge) });
    try {
      await controller.connect();
      const pending = controller.invoke("event/subscribe", subscribe("alice-private"), credentials("current-token"));
      await started;
      await controller.invoke("auth/login/status", { login_id: "bob-login" });
      release();
      await pending;
      initial.bridge.emitEvent(event("alice-private", "late-alice-event"));
      initial.bridge.emitExit();
      await vi.advanceTimersByTimeAsync(10);
      expect(events).not.toHaveBeenCalled();
      expect(replacement.requests.mock.calls.some(([method]) => method === "event/subscribe")).toBe(false);
    } finally { release(); await controller.close(); vi.useRealTimers(); }
  });
});
