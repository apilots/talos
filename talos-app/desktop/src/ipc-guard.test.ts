import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";

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

  it("allows llm config methods and rejects removed admin methods", () => {
    const removedMethod = "admin.session.list" as never;

    expect(isDesktopRendererMethod(removedMethod)).toBe(false);
    expect(isDesktopRendererMethod("admin.user.list")).toBe(false);
    expect(isDesktopRendererMethod("account.get")).toBe(false);
    expect(isDesktopRendererMethod("config.read")).toBe(true);
    expect(isDesktopRendererMethod("config.reload")).toBe(true);
    expect(() => assertDesktopRendererInvoke(removedMethod, {})).toThrow();
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

    expect(isDesktopRendererMethod("user.token.create")).toBe(false);
    expect(result.actorCredentials?.credential.type).toBe("existing_session_token");
    expect(isDesktopRendererMethod("auth.login")).toBe(false);
    expect(isDesktopRendererMethod("auth.register")).toBe(false);
  });

  it("allows desktop cloud session cache methods with validated shapes", () => {
    expect(() => assertDesktopRendererInvoke("auth.session.read", {})).not.toThrow();
    expect(() => assertDesktopRendererInvoke("auth.session.clear", {})).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke(
        "auth.session.store",
        {
          session: {
            schema_version: 1,
            cloud_api_base_url: "http://127.0.0.1:8787",
            access_token: "token-1",
            token_type: "Bearer",
            expires_at_ms: null,
            last_used_at_ms: Date.now().toString(),
            user: {
              user_id: "user-1",
              username: "ada",
              email: "ada@example.com",
              role: "admin",
            },
          },
        },
        {
          actorCredentials: {
            credential: {
              type: "existing_session_token",
              token: "token-1",
            },
          },
        },
      ),
    ).not.toThrow();
  });

  it("rejects renderer attempts to pass unsupported actor credentials", () => {
    expect(() =>
      assertDesktopRendererInvoke(
        "user.current",
        {},
        {
          actorCredentials: {
            credential: {
              type: "unsupported_token",
              token: "secret",
            },
          } as unknown as AppProtocol.ActorCredentials,
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
