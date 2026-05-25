import { describe, expect, it } from "vitest";

import { assertDesktopRendererInvoke, isDesktopRendererMethod } from "./ipc-guard";

describe("desktop IPC guard", () => {
  it("allows ordinary desktop renderer methods with valid params", () => {
    expect(() =>
      assertDesktopRendererInvoke("session.read", { session_id: "session-1" }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("session.history", { session_id: "session-1" }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("turn.start", {
        session_id: "session-1",
        input: [{ type: "text", text: "hello" }],
        workspace_id: null,
        client_request_id: "request-1",
      }),
    ).not.toThrow();
  });

  it("allows admin and llm config methods required by desktop admin UI", () => {
    expect(isDesktopRendererMethod("admin.session.list")).toBe(true);
    expect(isDesktopRendererMethod("admin.user.list")).toBe(true);
    expect(isDesktopRendererMethod("config.read")).toBe(true);
    expect(isDesktopRendererMethod("config.reload")).toBe(true);
    expect(() => assertDesktopRendererInvoke("admin.session.list", {})).not.toThrow();
    expect(() => assertDesktopRendererInvoke("config.value.write", {
      path: "llm",
      value: "{}",
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("config.batch.write", {
      entries: [{ path: "llm", value: "{}" }],
    })).not.toThrow();
  });

  it("allows shared user methods with real session credentials", () => {
    const result = assertDesktopRendererInvoke(
      "user.current",
      {},
      {
        actorCredentials: {
          credential: {
            type: "existing_session_token",
            token: "token-1",
          },
        },
      },
    );

    expect(isDesktopRendererMethod("user.token.create")).toBe(true);
    expect(result.actorCredentials?.credential.type).toBe("existing_session_token");
    expect(isDesktopRendererMethod("auth.login")).toBe(false);
    expect(isDesktopRendererMethod("auth.register")).toBe(false);
  });

  it("rejects renderer attempts to pass local launch credentials", () => {
    expect(() =>
      assertDesktopRendererInvoke(
        "user.current",
        {},
        {
          actorCredentials: {
            credential: {
              type: "local_launch_token",
              token: "secret",
            },
          },
        },
      ),
    ).toThrow("Invalid desktop renderer actor credentials.");
  });

  it("rejects initialize and unsupported config writes from renderer", () => {
    expect(isDesktopRendererMethod("initialize")).toBe(false);
    expect(() => assertDesktopRendererInvoke("config.value.write", {
      path: "storage",
      value: "{}",
    })).toThrow("Invalid desktop renderer params for method: config.value.write");
  });

  it("rejects malformed params before they reach app-server", () => {
    expect(() => assertDesktopRendererInvoke("session.read", {})).toThrow(
      "Invalid desktop renderer params for method: session.read",
    );
    expect(isDesktopRendererMethod("unsupported.method")).toBe(false);
    expect(() =>
      assertDesktopRendererInvoke("unsupported.method", {
        session_id: "session-1",
        call_id: "call-1",
        tool_name: "bash",
        approved: true,
      }),
    ).toThrow("Desktop renderer method is not allowed: unsupported.method");
    expect(() =>
      assertDesktopRendererInvoke("tool.dynamic_result.submit", {
        session_id: "session-1",
        call_id: "call-1",
        tool_name: "bash",
        output: "ok",
        success: "true",
      }),
    ).toThrow("Invalid desktop renderer params for method: tool.dynamic_result.submit");
  });
});
