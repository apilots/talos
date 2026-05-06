import { beforeEach, describe, expect, it } from "vitest";

import { applyActiveTurnSnapshot, applyStreamEvent } from "@/features/chat";
import { shouldRefreshSessionTitleAfterTurn } from "@/features/chat/hooks/useSessionQueryInvalidation";
import { shouldFinalizeDetachedStream } from "@/features/chat/hooks/useSessionReconnect";
import { buildSessionBlocks } from "@/features/chat/hooks/useSessionBlocks";
import { useAppStore } from "@/shared/store/app-store";
import {
  createDefaultProjects,
  describeProjectDirectory,
  getEffectiveSessionStatus,
  mapTurnsToBlocks,
  UNBOUND_PROJECT_ID,
} from "@/shared/types/domain";

const usage = {
  input_tokens: 0,
  output_tokens: 0,
  total_tokens: 0,
  cache_creation_input_tokens: null,
  cache_read_input_tokens: null,
};

function startedPayload() {
  return {
    type: "message_started" as const,
    request_id: "req-1",
    model: "test-model",
    usage,
    iteration: 0n,
    step_id: "step-1",
  };
}

function completedPayload() {
  return {
    type: "message_completed" as const,
    request_id: "req-1",
    stop_reason: "end_turn" as const,
    usage,
    total_output_items: 1n,
    iteration: 0n,
    step_id: "step-1",
    task_count: 0,
  };
}

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

  it("maps cancelled history turns into visible status context", () => {
    const blocks = mapTurnsToBlocks([
      {
        request_id: "req-1",
        message_id: "msg-1",
        session_id: "sess-1",
        model: "gpt-4.1",
        reasoning: [],
        stop_reason: "cancelled",
        usage,
        output: [{ type: "user_input", text: "stop after this input" }],
      },
    ]);

    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toMatchObject({ kind: "user", text: "stop after this input" });
    expect(blocks[1]).toMatchObject({ kind: "system", text: "Response cancelled." });
  });

  it("applies approval events into the transient stream store", () => {
    applyStreamEvent("sess-1", {
      event_type: "message_tool_call_pending_approval",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence: 3,
      payload: {
        type: "message_tool_call_pending_approval",
        request_id: "req-1",
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
        request_id: "req-1",
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
      payload: completedPayload(),
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
        request_id: "req-1",
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
      payload: startedPayload(),
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.activeMessageId).toBe("msg-new");
    expect(stream.lastSequence).toBe(0);
    expect(stream.connectionState).toBe("streaming");
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBe("running");
  });

  it("preserves pending user input when a stream event switches to the server message id", () => {
    useAppStore.getState().setPendingInput("sess-1", {
      text: "keep this user question visible",
      clientMessageId: "client-1",
    });
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "client-local",
      isStreaming: true,
      connectionState: "awaiting",
    });
    useAppStore.getState().appendStreamText("sess-1", "assistant", "stale response");

    applyStreamEvent("sess-1", {
      event_type: "message_started",
      message_id: "msg-server-1",
      session_id: "sess-1",
      sequence: 0,
      payload: startedPayload(),
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.pendingInput).toMatchObject({
      text: "keep this user question visible",
      clientMessageId: "client-1",
      serverMessageId: "msg-server-1",
    });
    expect(stream.blocks).toHaveLength(0);
    expect(stream.activeMessageId).toBe("msg-server-1");
  });

  it("preserves pending user input when active turn snapshots switch message ids", () => {
    useAppStore.getState().setPendingInput("sess-1", {
      text: "active turn should not hide me",
      clientMessageId: "client-2",
    });
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "msg-old",
      isStreaming: true,
      connectionState: "streaming",
    });
    useAppStore.getState().appendStreamText("sess-1", "assistant", "old response");

    applyActiveTurnSnapshot("sess-1", {
      session_id: "sess-1",
      turn_id: "turn-2",
      message_id: "msg-new",
      status: "running",
      last_event_id: "ev_00000000000000001000",
      pending_user_action: null,
      updated_at: 124,
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream.pendingInput).toMatchObject({
      text: "active turn should not hide me",
      clientMessageId: "client-2",
      serverMessageId: "msg-new",
    });
    expect(stream.blocks).toHaveLength(0);
    expect(stream.connectionState).toBe("detached");
  });

  it("renders pending input as a separate anchor before live response blocks", () => {
    const streamState = {
      isStreaming: true,
      connectionState: "streaming" as const,
      pendingInput: {
        text: "show the question while waiting",
        clientMessageId: "client-3",
      },
      blocks: [
        {
          id: "assistant-live-1",
          kind: "assistant" as const,
          text: "partial answer",
          live: true,
        },
      ],
    };

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState,
      turns: [],
      waitingResponseDetail: "Waiting detail",
      waitingResponseTitle: "Waiting",
      stoppingResponseDetail: "Stopping detail",
      stoppingResponseTitle: "Stopping",
    });

    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toMatchObject({
      kind: "user",
      text: "show the question while waiting",
    });
    expect(blocks[1]).toMatchObject({
      kind: "assistant",
      text: "partial answer",
    });
  });

  it("clears stale local stream state when active turn is completed", () => {
    useAppStore.getState().setPendingInput("sess-1", {
      text: "completed turn should keep pending input until history catches up",
      clientMessageId: "client-completed",
      serverMessageId: "msg-1",
    });
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
    expect(stream.pendingInput).toMatchObject({
      text: "completed turn should keep pending input until history catches up",
      serverMessageId: "msg-1",
    });
    expect(stream.showReconnectControls).toBeUndefined();
    expect(useAppStore.getState().transientStatuses["sess-1"]).toBeUndefined();
  });

  it("hides pending input once matching history user input is available", () => {
    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: {
        isStreaming: false,
        connectionState: "idle",
        pendingInput: {
          text: "history caught up",
          clientMessageId: "client-4",
          serverMessageId: "msg-1",
        },
        blocks: [],
      },
      turns: [
        {
          request_id: "req-1",
          message_id: "msg-1",
          session_id: "sess-1",
          model: "gpt-4.1",
          reasoning: [],
          stop_reason: "end_turn",
          usage,
          output: [
            { type: "user_input", text: "history caught up" },
            { type: "output_text", text: "done" },
          ],
        },
      ],
      waitingResponseDetail: "Waiting detail",
      waitingResponseTitle: "Waiting",
      stoppingResponseDetail: "Stopping detail",
      stoppingResponseTitle: "Stopping",
    });

    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toMatchObject({ kind: "user", text: "history caught up" });
    expect(blocks[1]).toMatchObject({ kind: "assistant", text: "done" });
  });

  it("hides completed pending input when history has the same user input without a matching message id", () => {
    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: {
        isStreaming: false,
        connectionState: "idle",
        pendingInput: {
          text: "same text committed through another message id",
          clientMessageId: "client-5",
          serverMessageId: "msg-pending",
        },
        blocks: [],
      },
      turns: [
        {
          request_id: "req-1",
          message_id: "msg-history",
          session_id: "sess-1",
          model: "gpt-4.1",
          reasoning: [],
          stop_reason: "end_turn",
          usage,
          output: [
            { type: "user_input", text: "same text committed through another message id" },
            { type: "output_text", text: "done" },
          ],
        },
      ],
      waitingResponseDetail: "Waiting detail",
      waitingResponseTitle: "Waiting",
      stoppingResponseDetail: "Stopping detail",
      stoppingResponseTitle: "Stopping",
    });

    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toMatchObject({
      kind: "user",
      text: "same text committed through another message id",
    });
    expect(blocks[1]).toMatchObject({ kind: "assistant", text: "done" });
  });

  it("keeps an active pending input visible when only an older history turn has the same text", () => {
    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: {
        isStreaming: true,
        connectionState: "awaiting",
        pendingInput: {
          text: "repeat prompt",
          clientMessageId: "client-6",
        },
        blocks: [],
      },
      turns: [
        {
          request_id: "req-1",
          message_id: "msg-history",
          session_id: "sess-1",
          model: "gpt-4.1",
          reasoning: [],
          stop_reason: "end_turn",
          usage,
          output: [
            { type: "user_input", text: "repeat prompt" },
            { type: "output_text", text: "older answer" },
          ],
        },
      ],
      waitingResponseDetail: "Waiting detail",
      waitingResponseTitle: "Waiting",
      stoppingResponseDetail: "Stopping detail",
      stoppingResponseTitle: "Stopping",
    });

    expect(blocks).toHaveLength(4);
    expect(blocks[0]).toMatchObject({ kind: "user", text: "repeat prompt" });
    expect(blocks[1]).toMatchObject({ kind: "assistant", text: "older answer" });
    expect(blocks[2]).toMatchObject({ kind: "user", text: "repeat prompt", live: true });
    expect(blocks[3]).toMatchObject({ kind: "status", title: "Waiting" });
  });

  it("clears the transient running state after message completion", () => {
    applyStreamEvent("sess-1", {
      event_type: "message_started",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence: 1,
      payload: startedPayload(),
    });

    applyStreamEvent("sess-1", {
      event_type: "message_completed",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence: 2,
      payload: completedPayload(),
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

  it("refreshes session titles only after the first turn still has the default title", () => {
    expect(shouldRefreshSessionTitleAfterTurn("Untitled Session", 0)).toBe(true);
    expect(shouldRefreshSessionTitleAfterTurn("Renamed session", 0)).toBe(false);
    expect(shouldRefreshSessionTitleAfterTurn("Untitled Session", 2)).toBe(false);
  });
});
