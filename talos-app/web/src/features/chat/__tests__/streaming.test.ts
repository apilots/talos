import { beforeEach, describe, expect, it } from "vitest";

import { applyStreamEvent } from "@/features/chat/store";
import { useAppStore } from "@/shared/store/app-store";
import {
  createDefaultProjects,
  describeProjectRoot,
  getEffectiveSessionStatus,
  mapTurnsToBlocks,
  resolveProjectRootPath,
} from "@/shared/types/domain";

describe("chat stream integration helpers", () => {
  beforeEach(() => {
    useAppStore.setState((state) => ({
      ...state,
      streams: {},
      transientStatuses: {},
    }));
  });

  it("maps history turns into ordered render blocks", () => {
    const blocks = mapTurnsToBlocks([
      {
        request_id: "req-1",
        message_id: "msg-1",
        session_id: "sess-1",
        model: "gpt-4.1",
        reasoning: [],
        stop_reason: "end_turn",
        usage: {
          input_tokens: 1,
          output_tokens: 1,
          total_tokens: 2,
        },
        output: [
          { type: "user_input", text: "hello" },
          { type: "output_text", text: "world" },
          {
            type: "tool_call",
            call_id: "call-1",
            name: "read_file",
            arguments: { path: "README.md" },
          },
          {
            type: "tool_result",
            call_id: "call-1",
            tool_name: "read_file",
            content: [{ type: "text", text: "done" }],
          },
        ],
      },
    ]);

    expect(blocks).toHaveLength(3);
    expect(blocks[0]).toMatchObject({ kind: "user", text: "hello" });
    expect(blocks[1]).toMatchObject({ kind: "assistant", text: "world" });
    expect(blocks[2]).toMatchObject({
      kind: "tool",
      callId: "call-1",
      output: "done",
      status: "done",
    });
  });

  it("applies approval events into the transient stream store", () => {
    applyStreamEvent("sess-1", {
      event_type: "message_tool_call_pending_approval",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence: 3,
      payload: {
        type: "message_tool_call_pending_approval",
        call_id: "call-1",
        tool_name: "exec_command",
        prompt: "Approve command?",
        arguments: { cmd: "cargo test" },
        step_id: "step-1",
      },
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.pendingApproval).toMatchObject({
      callId: "call-1",
      toolName: "exec_command",
    });
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBe("waiting_approval");
  });

  it("clears the transient running state after message completion", () => {
    applyStreamEvent("sess-1", {
      event_type: "message_started",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence: 1,
      payload: {
        type: "message_started",
      },
    });

    applyStreamEvent("sess-1", {
      event_type: "message_completed",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence: 2,
      payload: {
        type: "message_completed",
      },
    });

    const state = useAppStore.getState();
    expect(state.transientStatuses["sess-1"]).toBeUndefined();
    expect(getEffectiveSessionStatus(state.transientStatuses["sess-1"], state.streams["sess-1"])).toBe("done");
  });

  it("keeps detached streams visible as running sessions", () => {
    useAppStore.getState().setStreamMeta("sess-1", {
      isStreaming: true,
      connectionState: "detached",
      expectedMessageCount: 4,
    });

    const state = useAppStore.getState();
    expect(getEffectiveSessionStatus(state.transientStatuses["sess-1"], state.streams["sess-1"])).toBe("running");
    expect(state.streams["sess-1"]?.connectionState).toBe("detached");
  });

  it("uses the configured default workspace path for web sessions", () => {
    const projects = createDefaultProjects();
    const defaultWorkspace = projects[0];

    expect(resolveProjectRootPath(defaultWorkspace.root)).toBe(__TALOS_DEFAULT_PROJECT_ROOT__);
    expect(describeProjectRoot(defaultWorkspace.root)).toBe(__TALOS_DEFAULT_PROJECT_ROOT__);
  });
});
