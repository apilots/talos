import type { TalosSession } from "@talos/sdk-browser";
import { describe, expect, it } from "vitest";

import { filterProjectSessions } from "./useProjectSessionsPageState";

describe("useProjectSessionsPageState helpers", () => {
  it("filters sessions by title and keeps newest first", () => {
    const sessions: TalosSession[] = [
      {
        title: "Refactor router",
        context: {
          session_id: "session-1",
          source_type: "http",
          provider: "http",
          user_id: "user-1",
          message_id: "message-1",
          trace_id: "trace-1",
        },
        conf: {},
        config: {
          agent: { model: "qwen-plus" },
          llm: { model: "qwen-plus" },
        },
        state: {
          created_at: 1,
          last_activity: 20,
          message_count: 1,
          usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0 },
        },
        project_id: "project-1",
      },
      {
        title: "Fix memory flush",
        context: {
          session_id: "session-2",
          source_type: "http",
          provider: "http",
          user_id: "user-1",
          message_id: "message-2",
          trace_id: "trace-2",
        },
        conf: {},
        config: {
          agent: { model: "gpt-5.4" },
          llm: { model: "gpt-5.4" },
        },
        state: {
          created_at: 1,
          last_activity: 10,
          message_count: 1,
          usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0 },
        },
        project_id: "project-1",
      },
    ];

    const filtered = filterProjectSessions(sessions, "router");

    expect(filtered).toHaveLength(1);
    expect(filtered[0]?.context.session_id).toBe("session-1");
  });
});
