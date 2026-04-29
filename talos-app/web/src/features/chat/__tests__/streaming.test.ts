import { beforeEach, describe, expect, it } from "vitest";

import { applyActiveTurnSnapshot, applyStreamEvent } from "@/features/chat";
import { shouldFinalizeDetachedStream } from "@/features/chat/hooks/useSessionReconnect";
import { useAppStore } from "@/shared/store/app-store";
import {
  createDefaultProjects,
  describeProjectDirectory,
  getEffectiveSessionStatus,
  mapTurnsToBlocks,
  UNBOUND_PROJECT_ID,
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
            name: "Read",
            arguments: { path: "README.md" },
          },
          {
            type: "tool_result",
            call_id: "call-1",
            tool_name: "Read",
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
        tool_name: "Bash",
        prompt: "Approve command?",
        arguments: { cmd: "cargo test" },
        step_id: "step-1",
      },
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.pendingApproval).toMatchObject({
      callId: "call-1",
      toolName: "Bash",
    });
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBe("waiting_approval");
  });

  it("keeps pending approval visible after message completion", () => {
    applyStreamEvent("sess-1", {
      event_type: "message_tool_call_pending_approval",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence: 3,
      payload: {
        type: "message_tool_call_pending_approval",
        call_id: "call-1",
        tool_name: "Agent",
        prompt: "Approve agent task?",
        arguments: { description: "Count rs files" },
        step_id: "step-1",
      },
    });

    applyStreamEvent("sess-1", {
      event_type: "message_completed",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence: 4,
      payload: {
        type: "message_completed",
      },
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream?.pendingApproval).toMatchObject({
      callId: "call-1",
      toolName: "Agent",
    });
    expect(stream?.connectionState).toBe("idle");
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBe("waiting_approval");
  });

  it("applies dynamic tool pending events into the transient stream store", () => {
    applyStreamEvent("sess-1", {
      event_type: "message_tool_call_pending_dynamic_result",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence: 3,
      payload: {
        type: "message_tool_call_pending_dynamic_result",
        call_id: "call-1",
        tool_name: "ExternalTool",
        arguments: { query: "status" },
        step_id: "step-1",
      },
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.pendingUserAction).toMatchObject({
      type: "dynamic_tool_result",
      call_id: "call-1",
      tool_name: "ExternalTool",
    });
    expect(stream.blocks).toContainEqual(
      expect.objectContaining({
        kind: "tool",
        callId: "call-1",
        toolName: "ExternalTool",
        status: "running",
      }),
    );
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBe("running");
  });

  it("hydrates pending approval from an active turn snapshot", () => {
    applyActiveTurnSnapshot("sess-1", {
      session_id: "sess-1",
      turn_id: "turn-1",
      message_id: "msg-1",
      status: "waiting_user_action",
      last_event_id: "ev_00000000000000000004",
      pending_user_action: {
        type: "tool_approval",
        call_id: "call-1",
        tool_name: "Bash",
        prompt: "Approve command?",
        arguments: { cmd: "date" },
      },
      updated_at: 123,
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.pendingApproval).toMatchObject({
      callId: "call-1",
      toolName: "Bash",
    });
    expect(stream.pendingUserAction).toMatchObject({
      type: "tool_approval",
      call_id: "call-1",
    });
    expect(stream.connectionState).toBe("detached");
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBe("waiting_approval");
  });

  it("hydrates running active turn snapshots for reconnect replay", () => {
    applyActiveTurnSnapshot("sess-1", {
      session_id: "sess-1",
      turn_id: "turn-1",
      message_id: "msg-1",
      status: "running",
      last_event_id: "ev_00000000000000000004",
      pending_user_action: null,
      updated_at: 123,
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.activeMessageId).toBe("msg-1");
    expect(stream.isStreaming).toBe(true);
    expect(stream.connectionState).toBe("detached");
    expect(stream.reconnectAttempt).toBe(0);
    expect(stream.showReconnectControls).toBe(false);
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBe("running");
  });

  it("clears stale per-message sequence when a running snapshot points to a new message", () => {
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "msg-old",
      lastSequence: 918,
      isStreaming: false,
      connectionState: "idle",
    });

    applyActiveTurnSnapshot("sess-1", {
      session_id: "sess-1",
      turn_id: "turn-2",
      message_id: "msg-new",
      status: "running",
      last_event_id: "ev_00000000000000001000",
      pending_user_action: null,
      updated_at: 124,
    });

    applyStreamEvent("sess-1", {
      event_type: "message_started",
      message_id: "msg-new",
      session_id: "sess-1",
      sequence: 0,
      payload: {
        type: "message_started",
      },
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.activeMessageId).toBe("msg-new");
    expect(stream.lastSequence).toBe(0);
    expect(stream.connectionState).toBe("streaming");
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBe("running");
  });

  it("clears stale local stream state when active turn is completed", () => {
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "msg-1",
      isStreaming: true,
      connectionState: "detached",
    });
    useAppStore.getState().setSessionStatus("sess-1", "running");

    applyActiveTurnSnapshot("sess-1", {
      session_id: "sess-1",
      turn_id: "turn-1",
      message_id: "msg-1",
      status: "completed",
      last_event_id: "ev_00000000000000000004",
      pending_user_action: null,
      updated_at: 123,
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.isStreaming).toBe(false);
    expect(stream.connectionState).toBe("idle");
    expect(stream.blocks).toHaveLength(0);
    expect(stream.showReconnectControls).toBeUndefined();
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBeUndefined();
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

  it("only finalizes by server message count after stream detaches", () => {
    const liveStream = {
      isStreaming: true,
      connectionState: "streaming" as const,
      expectedMessageCount: 3,
      blocks: [{ id: "user-live", kind: "user" as const, text: "hello", live: true }],
    };

    expect(shouldFinalizeDetachedStream(liveStream, 3)).toBe(false);
    expect(
      shouldFinalizeDetachedStream(
        { ...liveStream, connectionState: "detached" },
        3,
      ),
    ).toBe(true);
    expect(
      shouldFinalizeDetachedStream(
        { ...liveStream, connectionState: "reconnecting" },
        3,
      ),
    ).toBe(true);
  });

  it("keeps an explicit unbound project option for web sessions", () => {
    const projects = createDefaultProjects();
    const defaultProject = projects[0];

    expect(defaultProject?.id).toBe(UNBOUND_PROJECT_ID);
    expect(describeProjectDirectory(defaultProject?.directory)).toBe("No project");
  });
});
