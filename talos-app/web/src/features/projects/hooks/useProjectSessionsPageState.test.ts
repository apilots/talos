import type { SessionInfo } from "@talos/sdk-browser";
import { describe, expect, it } from "vitest";

import { filterProjectSessions } from "./useProjectSessionsPageState";

describe("useProjectSessionsPageState helpers", () => {
  it("filters sessions by title and keeps newest first", () => {
    const sessions: SessionInfo[] = [
      {
        id: "session-1",
        title: "Refactor router",
        user_id: "user-1",
        source_type: "http",
        provider: "http",
        model: "qwen-plus",
        created_at: 1,
        last_activity: 20,
        message_count: 1,
        usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0 },
        project_id: "project-1",
      },
      {
        id: "session-2",
        title: "Fix memory flush",
        user_id: "user-1",
        source_type: "http",
        provider: "http",
        model: "gpt-5.4",
        created_at: 1,
        last_activity: 10,
        message_count: 1,
        usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0 },
        project_id: "project-1",
      },
    ];

    const filtered = filterProjectSessions(sessions, "router");

    expect(filtered).toHaveLength(1);
    expect(filtered[0]?.id).toBe("session-1");
  });
});
