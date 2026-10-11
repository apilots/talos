import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";

import { assertDesktopRendererInvoke, isDesktopRendererMethod } from "./ipc-guard";

describe("desktop IPC guard", () => {
  it("allows Computer metadata operations while keeping credentials in the trusted host", () => {
    expect(() => assertDesktopRendererInvoke("computer/list", { query: "laptop", limit: 50 })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("computer/read", { nodeId: "node-1" })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("computer/rename", { nodeId: "node-1", displayName: "Laptop" })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("computer/setStatus", { nodeId: "node-1", status: "disabled" })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("computer/create", { displayName: "Laptop", credentialExpiresAt: 2000000000 })).toThrow();
    expect(() => assertDesktopRendererInvoke("computer/rotateCredential", { nodeId: "node-1", expiresAt: 2000000000 })).toThrow();
  });
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
      assertDesktopRendererInvoke("session/get", { sessionId: "session-1" }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("session/history", { sessionId: "session-1" }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("event/unsubscribe", { subscriptionId: "subscription-1" }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("turn/start", {
        sessionId: "session-1",
        input: { content: [{ type: "text", text: "hello" }] },
        clientRequestId: "request-1",
      }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("fs/directory/read", { path: null }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("fs/directory/create", {
        parent: "/Users/example",
        name: "project",
      }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("memory/get", { memoryId: "memory-1" }),
    ).not.toThrow();
    expect(() =>
      assertDesktopRendererInvoke("turn/input", {
        identity: { sessionId: "session-1", turnId: "turn-1" },
        input: {
          type: "userActionResponse",
          actionId: "action-1",
          response: {
            type: "question",
            response: { type: "selectedOption", optionId: "now" },
          },
        },
        clientRequestId: null,
      }),
    ).not.toThrow();
  });

  it("allows message queue methods with transport-level params", () => {
    expect(() => assertDesktopRendererInvoke("messageQueue/add", {
      sessionId: "session-1",
      input: { content: [{ type: "text", text: "queued" }] },
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("messageQueue/list", {
      sessionId: "session-1",
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("messageQueue/update", {
      sessionId: "session-1",
      messageId: "q1",
      input: { content: [{ type: "text", text: "updated" }] },
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("messageQueue/delete", {
      sessionId: "session-1",
      messageId: "q1",
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("messageQueue/reorder", {
      sessionId: "session-1",
      messageIds: ["q2", "q1"],
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("messageQueue/start", {
      sessionId: "session-1",
      messageId: null,
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("turn/input", {
      identity: { sessionId: "session-1", turnId: "turn-1" },
      input: { type: "queuedMessage", messageId: "q1" },
      clientRequestId: null,
    })).not.toThrow();
  });

  it("allows llm config methods", () => {
    expect(isDesktopRendererMethod("config/get")).toBe(true);
    expect(isDesktopRendererMethod("config/reload")).toBe(true);
    expect(() => assertDesktopRendererInvoke("config/get", {
      path: "webSearch",
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("config/update", {
      value: { type: "llm", value: {} },
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("config/update", {
      value: { type: "webSearch", value: {} },
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("config/delete", {
      path: "llm",
    })).not.toThrow();
  });

  it("allows shared user methods with real session credentials", () => {
    const result = assertDesktopRendererInvoke(
      "user/current",
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

    expect(isDesktopRendererMethod("user/token/create")).toBe(false);
    expect(result.actorCredentials?.credential.type).toBe("existing_session_token");
  });

  it("allows profile reads with empty parameters", () => {
    expect(() => assertDesktopRendererInvoke("profile/read", {})).not.toThrow();
  });

  it("allows desktop cloud session cache read and clear methods", () => {
    expect(() => assertDesktopRendererInvoke("auth/session/read", {})).not.toThrow();
    expect(() => assertDesktopRendererInvoke("auth/session/clear", {})).not.toThrow();
  });

  it("rejects renderer attempts to pass unsupported actor credentials", () => {
    expect(() =>
      assertDesktopRendererInvoke(
        "user/current",
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
    expect(() => assertDesktopRendererInvoke("config/update", {
      value: { type: "llm", value: {} },
    })).not.toThrow();
    expect(() => assertDesktopRendererInvoke("config/update", {
      value: { type: "storage", value: {} },
    })).toThrow("Invalid desktop renderer params for method: config/update");
  });

  it("rejects malformed params before they reach app-server", () => {
    expect(() => assertDesktopRendererInvoke("session/get", {})).toThrow(
      "Invalid desktop renderer params for method: session/get",
    );
    expect(isDesktopRendererMethod("unsupported/method")).toBe(false);
    expect(() =>
      assertDesktopRendererInvoke("unsupported/method", {
        sessionId: "session-1",
        callId: "call-1",
        toolName: "bash",
        approved: true,
      }),
    ).toThrow("Desktop renderer method is not allowed: unsupported/method");
    expect(() =>
      assertDesktopRendererInvoke("fs/directory/create", {
        parent: "/Users/example",
      }),
    ).toThrow("Invalid desktop renderer params for method: fs/directory/create");
    expect(() =>
      assertDesktopRendererInvoke("turn/input", {
        identity: { sessionId: "session-1", turnId: "turn-1" },
        input: { type: "userActionResponse", actionId: "action-1", response: { type: "unknown" } },
        clientRequestId: null,
      }),
    ).toThrow("Invalid desktop renderer params for method: turn/input");
  });

  it("allows read-only workspace inspection with empty parameters", () => {
    expect(isDesktopRendererMethod("workspace/info")).toBe(true);
    expect(() => assertDesktopRendererInvoke("workspace/info", {})).not.toThrow();
    expect(() => assertDesktopRendererInvoke("workspace/info", null)).toThrow();
  });
});
