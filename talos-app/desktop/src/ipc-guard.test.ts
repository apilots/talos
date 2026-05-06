import { describe, expect, it } from "vitest";

import { assertDesktopRendererInvoke, isDesktopRendererMethod } from "./ipc-guard";

describe("desktop IPC guard", () => {
  it("allows ordinary desktop renderer methods with valid params", () => {
    expect(() =>
      assertDesktopRendererInvoke("session.read", { session_id: "session-1" }),
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
    expect(() => assertDesktopRendererInvoke("admin.session.list", {})).not.toThrow();
    expect(() => assertDesktopRendererInvoke("config.value.write", {
      path: "llm",
      value: "{}",
    })).not.toThrow();
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
    expect(() =>
      assertDesktopRendererInvoke("tool.approval.submit", {
        session_id: "session-1",
        call_id: "call-1",
        tool_name: "bash",
        approved: "true",
      }),
    ).toThrow("Invalid desktop renderer params for method: tool.approval.submit");
  });
});
