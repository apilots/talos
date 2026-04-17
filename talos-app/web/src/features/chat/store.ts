import type { TalosEvent, TalosEventPayload } from "@talos/sdk";

import { useAppStore } from "@/shared/store/app-store";
import { outputContentToText } from "@/shared/types/domain";

function resolvePayloadType(payload: TalosEvent["payload"]): TalosEventPayload["type"] {
  return payload.type;
}

export function applyStreamEvent(sessionId: string, event: TalosEvent) {
  const state = useAppStore.getState();
  const stream = state.streams[sessionId];

  if (stream?.lastSequence !== undefined && event.sequence < stream.lastSequence) {
    return;
  }

  state.setStreamMeta(sessionId, {
    lastSequence: event.sequence,
    activeMessageId: event.message_id,
    isStreaming: true,
    connectionState: "streaming",
  });

  const payloadType = resolvePayloadType(event.payload);
  switch (payloadType) {
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
    case "message_tool_call_completed":
      state.completeStreamTool(
        sessionId,
        String(event.payload.call_id),
        outputContentToText((event.payload.content ?? []) as never[]),
        event.payload.is_error ? "error" : "done",
        Number(event.payload.timing_ms ?? 0),
      );
      return;
    case "message_completed":
      state.setPendingApproval(sessionId, undefined);
      state.setSessionStatus(sessionId, undefined);
      state.setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        expectedMessageCount: undefined,
      });
      return;
    case "message_error":
      state.setPendingApproval(sessionId, undefined);
      state.setSessionStatus(sessionId, "error");
      state.setStreamError(sessionId, String(event.payload.error ?? "Unknown stream error."));
      return;
    case "message_tool_call_args_delta":
      return;
  }
}
