import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";

import { assertDesktopRendererInvoke, isDesktopRendererMethod } from "./ipc-guard";

describe("desktop IPC guard", () => {
  it("allows typed file methods without duplicating file business validation", () => {
    expect(() =>
      assertDesktopRendererInvoke("file/upload", {
        request: {
          filename: "diagram.png",
          scope: { type: "personal" },
          tags: [],
          metadata: {},
        },
        dataBase64: "iVBORw==",
      }),
    ).not.toThrow();
    expect(() => assertDesktopRendererInvoke("file/list", {})).not.toThrow();
    expect(() => assertDesktopRendererInvoke("file/read", { fileId: "file-1" })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("file/update", { fileId: "file-1", tags: [] })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("file/delete", { fileId: "file-1" })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("file/read", {})).toThrow(
      "Invalid desktop renderer params for method: file/read",
    );
  });

  it("allows ordinary desktop renderer methods with valid params", () => {
    expect(() =>
      assertDesktopRendererInvoke("session.get", { session_id: "session-1" }),
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
    expect(() =>
      assertDesktopRendererInvoke("fs.directory.read", { path: null }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("fs.directory.create", {
        parent: "/Users/example",
        name: "project",
      }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("memory/get", { memoryId: "memory-1" }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("userAction/submit", {
        sessionId: "session-1",
        turnId: "turn-1",
        actionId: "action-1",
        response: {
          type: "question",
          response: { type: "selectedOption", optionId: "now" },
        },
      }),
    ).not.toThrow();
  });

  it("allows llm config methods", () => {
    expect(isDesktopRendererMethod("config.get")).toBe(true);
    expect(isDesktopRendererMethod("config.reload")).toBe(true);
    expect(() => assertDesktopRendererInvoke("config.update", {
      path: "llm",
      value: { type: "llm", value: {} },
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("config.delete", {
      path: "llm",
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
  });

  it("allows profile reads with empty parameters", () => {
    expect(() => assertDesktopRendererInvoke("profile/read", {})).not.toThrow();
  });

  it("allows desktop cloud session cache read and clear methods", () => {
    expect(() => assertDesktopRendererInvoke("auth.session.read", {})).not.toThrow();
    expect(() => assertDesktopRendererInvoke("auth.session.clear", {})).not.toThrow();
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
    expect(() => assertDesktopRendererInvoke("config.update", {
      path: "storage",
      value: { type: "llm", value: {} },
    })).toThrow("Invalid desktop renderer params for method: config.update");
  });

  it("rejects malformed params before they reach app-server", () => {
    expect(() => assertDesktopRendererInvoke("session.get", {})).toThrow(
      "Invalid desktop renderer params for method: session.get",
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
    expect(() =>
      assertDesktopRendererInvoke("fs.directory.create", {
        parent: "/Users/example",
      }),
    ).toThrow("Invalid desktop renderer params for method: fs.directory.create");
    expect(() =>
      assertDesktopRendererInvoke("userAction/submit", {
        sessionId: "session-1",
        turnId: "turn-1",
        actionId: "action-1",
        response: { type: "unknown" },
      }),
    ).toThrow("Invalid desktop renderer params for method: userAction/submit");
  });
});
