import type { TalosActiveTurn, TalosEvent } from "@talos/sdk-core";

import { useAppStore } from "@/shared/store/app-store";
import { outputContentToText } from "@/shared/types/domain";

export function applyStreamEvent(sessionId: string, event: TalosEvent) {
  const state = useAppStore.getState();
  const stream = state.streams[sessionId];
  const isSameMessage = stream?.activeMessageId === event.message_id;

  if (isSameMessage && stream?.lastSequence !== undefined && event.sequence <= stream.lastSequence) {
    return;
  }

  if (stream?.activeMessageId && !isSameMessage) {
    state.resetLiveResponse(sessionId);
  }

  if (stream?.pendingInput && stream.pendingInput.serverMessageId !== event.message_id) {
    state.setPendingInput(sessionId, {
      ...stream.pendingInput,
      serverMessageId: event.message_id,
    });
  }

  state.setStreamMeta(sessionId, {
    lastSequence: event.sequence,
    activeMessageId: event.message_id,
    isStreaming: true,
    connectionState: "streaming",
    reconnectAttempt: undefined,
    showReconnectControls: false,
    awaitingFinalResponse: false,
  });

  switch (event.payload.type) {
    case "message_started":
      state.setSessionStatus(sessionId, "running");
      return;
    case "message_reasoning_delta":
      state.appendStreamText(sessionId, "thinking", String(event.payload.delta ?? ""));
      return;
    case "message_execution_summary":
      state.appendStreamText(sessionId, "system", String(event.payload.text ?? ""));
      return;
    case "message_output_delta":
      state.appendStreamText(sessionId, "assistant", String(event.payload.delta ?? ""));
      return;
    case "message_tool_call_started":
      state.upsertStreamTool(sessionId, String(event.payload.call_id), {
        toolName: String(event.payload.name),
        arguments: event.payload.arguments,
        status: "queued",
      });
      return;
    case "message_tool_call_queued":
      state.upsertStreamTool(sessionId, String(event.payload.call_id), {
        toolName: String(event.payload.tool_name),
        status: "queued",
      });
      return;
    case "message_tool_call_executing":
      state.upsertStreamTool(sessionId, String(event.payload.call_id), {
        toolName: String(event.payload.tool_name),
        status: "running",
      });
      state.setSessionStatus(sessionId, "running");
      return;
    case "message_tool_call_progress_delta":
      state.appendToolProgress(
        sessionId,
        String(event.payload.call_id),
        String(event.payload.message),
        event.payload.progress_kind as "status" | "stdout" | "stderr",
      );
      return;
    case "message_tool_call_pending_approval":
      state.setPendingUserAction(sessionId, {
        type: "tool_approval",
        call_id: String(event.payload.call_id),
        tool_name: String(event.payload.tool_name),
        prompt: String(event.payload.prompt),
        arguments: event.payload.arguments,
      });
      state.upsertStreamTool(sessionId, String(event.payload.call_id), {
        toolName: String(event.payload.tool_name),
        arguments: event.payload.arguments,
        prompt: String(event.payload.prompt),
        status: "waiting_approval",
      });
      state.setPendingApproval(sessionId, {
        callId: String(event.payload.call_id),
        toolName: String(event.payload.tool_name),
        prompt: String(event.payload.prompt),
        arguments: event.payload.arguments,
      });
      state.setSessionStatus(sessionId, "waiting_approval");
      return;
    case "message_tool_call_pending_dynamic_result":
      state.setPendingUserAction(sessionId, {
        type: "dynamic_tool_result",
        call_id: String(event.payload.call_id),
        tool_name: String(event.payload.tool_name),
      });
      state.upsertStreamTool(sessionId, String(event.payload.call_id), {
        toolName: String(event.payload.tool_name),
        arguments: event.payload.arguments,
        status: "running",
      });
      state.appendToolProgress(
        sessionId,
        String(event.payload.call_id),
        "Waiting for dynamic tool result.",
        "status",
      );
      state.setSessionStatus(sessionId, "running");
      return;
    case "message_tool_call_completed":
      state.completeStreamTool(
        sessionId,
        String(event.payload.call_id),
        outputContentToText((event.payload.content ?? []) as never[]),
        event.payload.is_error ? "error" : "done",
        Number(event.payload.timing_ms ?? 0),
      );
      state.setStreamMeta(sessionId, { awaitingFinalResponse: true });
      return;
    case "message_completed":
      state.setSessionStatus(
        sessionId,
        stream?.pendingApproval ? "waiting_approval" : stream?.pendingUserAction ? "running" : undefined,
      );
      state.setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        expectedMessageCount: undefined,
        awaitingFinalResponse: false,
      });
      return;
    case "message_error":
      state.setPendingUserAction(sessionId, undefined);
      state.setPendingApproval(sessionId, undefined);
      state.setSessionStatus(sessionId, "error");
      state.setStreamError(sessionId, String(event.payload.error ?? "Unknown stream error."));
      return;
    case "message_tool_call_args_delta":
      return;
  }
}

export function applyActiveTurnSnapshot(sessionId: string, activeTurn: TalosActiveTurn) {
  if (activeTurn.status === "completed") {
    const state = useAppStore.getState();
    state.setPendingUserAction(sessionId, undefined);
    state.setPendingApproval(sessionId, undefined);
    state.setSessionStatus(sessionId, undefined);
    state.setStreamMeta(sessionId, {
      activeMessageId: activeTurn.message_id ?? undefined,
      isStreaming: false,
      connectionState: "idle",
      expectedMessageCount: undefined,
      reconnectAttempt: undefined,
      showReconnectControls: undefined,
      awaitingFinalResponse: false,
    });
    return;
  }

  if (activeTurn.status === "failed") {
    const state = useAppStore.getState();
    state.setPendingUserAction(sessionId, undefined);
    state.setPendingApproval(sessionId, undefined);
    state.setSessionStatus(sessionId, "error");
    state.setStreamMeta(sessionId, {
      isStreaming: false,
      connectionState: "idle",
    });
    return;
  }

  const pending = activeTurn.pending_user_action;
  if (activeTurn.status === "running" && activeTurn.message_id) {
    const state = useAppStore.getState();
    const stream = state.streams[sessionId];
    const messageChanged = stream?.activeMessageId !== activeTurn.message_id;
    if (stream?.activeMessageId && messageChanged) {
      state.resetLiveResponse(sessionId);
    }
    if (stream?.pendingInput && stream.pendingInput.serverMessageId !== activeTurn.message_id) {
      state.setPendingInput(sessionId, {
        ...stream.pendingInput,
        serverMessageId: activeTurn.message_id,
      });
    }
    state.setStreamMeta(sessionId, {
      activeMessageId: activeTurn.message_id,
      lastSequence: messageChanged ? undefined : stream?.lastSequence,
      isStreaming: true,
      connectionState: "detached",
      reconnectAttempt: 0,
      showReconnectControls: false,
    });
    state.setSessionStatus(sessionId, "running");
    return;
  }

  if (activeTurn.status !== "waiting_user_action" || !pending) {
    return;
  }

  const state = useAppStore.getState();
  state.setPendingUserAction(sessionId, pending);
  state.setStreamMeta(sessionId, {
    activeMessageId: activeTurn.message_id ?? undefined,
    isStreaming: true,
    connectionState: "detached",
  });

  if (pending.type === "tool_approval") {
    state.upsertStreamTool(sessionId, pending.call_id, {
      toolName: pending.tool_name,
      arguments: pending.arguments,
      prompt: pending.prompt,
      status: "waiting_approval",
    });
    state.setPendingApproval(sessionId, {
      callId: pending.call_id,
      toolName: pending.tool_name,
      prompt: pending.prompt,
      arguments: pending.arguments,
    });
    state.setSessionStatus(sessionId, "waiting_approval");
    return;
  }

  state.upsertStreamTool(sessionId, pending.call_id, {
    toolName: pending.tool_name,
    status: "running",
  });
  state.appendToolProgress(
    sessionId,
    pending.call_id,
    "Waiting for dynamic tool result.",
    "status",
  );
  state.setSessionStatus(sessionId, "running");
}
