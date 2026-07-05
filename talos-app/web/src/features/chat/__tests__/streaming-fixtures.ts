import type {
  TalosActiveTurn,
  TalosContentItem,
  TalosSessionHistorySnapshot,
  AgentEvent,
  TimelineEventEnvelope,
} from "@talos/sdk-core";

export function baseEvent(payload: AgentEvent["payload"], sequence: number): TimelineEventEnvelope {
  return {
    event_seq: sequence,
    event_id: `ev_${sequence}`,
    created_at_ms: sequence,
    turn_id: "turn-1",
    message_id: "msg-1",
    session_id: "sess-1",
    event: {
      event_id: `ev_${sequence}`,
      event_seq: sequence,
      created_at_ms: sequence,
      event_type: payload.type,
      turn_id: "turn-1",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence,
      payload,
    },
  };
}

export function itemStarted(
  itemId: string,
  kind: "assistant_text" | "tool_call" | "user_input" | "reasoning",
  phase: "commentary" | "final_answer" | "unknown" | "acting" | "input" | "thinking",
  sequence: number,
): TimelineEventEnvelope {
  return baseEvent({
    type: "item_started",
    request_id: "req-1",
    turn_id: "turn-1",
    item: {
      item_id: itemId,
      parent_item_id: null,
      kind,
      phase,
      status: "running",
      title: kind === "tool_call" ? "Shell" : null,
      started_at_ms: 1,
      completed_at_ms: null,
      content: kind === "tool_call"
        ? {
            type: "tool_call",
            call_id: "call-1",
            name: "Shell",
            arguments: "{}",
            progress: [],
            result: null,
          }
        : { type: "text", text: "" },
    },
  }, sequence);
}

export function withMessage(event: TimelineEventEnvelope, messageId: string, turnId: string): TimelineEventEnvelope {
  return {
    ...event,
    turn_id: turnId,
    message_id: messageId,
    event: {
      ...event.event,
      turn_id: turnId,
      message_id: messageId,
    },
  };
}

export function assistantTextCommitted(
  itemId: string,
  phase: "commentary" | "final_answer",
  sequence: number,
): TimelineEventEnvelope {
  return baseEvent({
    type: "assistant_text_committed",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    phase,
  }, sequence);
}

export function userInputStarted(itemId: string, text: string, sequence: number): TimelineEventEnvelope {
  const event = itemStarted(itemId, "user_input", "input", sequence);
  if (event.event.payload.type === "item_started") {
    event.event.payload.item.content = { type: "text", text };
  }
  return event;
}

export function itemCompleted(itemId: string, sequence: number): TimelineEventEnvelope {
  return baseEvent({
    type: "item_completed",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    status: "completed",
    completed_at_ms: sequence,
    final_content: null,
  }, sequence);
}

export function textDelta(itemId: string, text: string, sequence: number): TimelineEventEnvelope {
  return baseEvent({
    type: "item_delta",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    delta: { type: "text", text },
  }, sequence);
}

export function toolProgress(itemId: string, text: string, sequence: number): TimelineEventEnvelope {
  return baseEvent({
    type: "item_delta",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    delta: { type: "tool_progress", progress_kind: "status", text_delta: text },
  }, sequence);
}

export function toolResult(
  itemId: string,
  content: string | TalosContentItem[],
  sequence: number,
): TimelineEventEnvelope {
  return baseEvent({
    type: "item_delta",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    delta: {
      type: "tool_result",
      content: typeof content === "string" ? [{ type: "text", text: content }] : content,
      is_error: false,
    },
  }, sequence);
}

export function contextCompactionStarted(
  itemId: string,
  stage: "preparing" | "summarizing" | "rebuilding_prompt" | "trimming" | "completed" | "failed",
  message: string | null,
  sequence: number,
): TimelineEventEnvelope {
  return baseEvent({
    type: "item_started",
    request_id: "req-1",
    turn_id: "turn-1",
    item: {
      item_id: itemId,
      parent_item_id: null,
      kind: "context_compaction",
      phase: "system",
      status: "running",
      title: null,
      started_at_ms: sequence,
      completed_at_ms: null,
      content: {
        type: "context_compaction",
        trigger: "threshold_exceeded",
        stage,
        outcome: null,
        metrics: null,
        message,
      },
    },
  }, sequence);
}

export function contextCompactionDelta(
  itemId: string,
  stage: "preparing" | "summarizing" | "rebuilding_prompt" | "trimming" | "completed" | "failed",
  outcome: "applied" | "rejected_missing_summary" | "trimmed_fallback_applied" | "failed_context_still_too_large" | "failed_provider_error" | null,
  message: string | null,
  sequence: number,
): TimelineEventEnvelope {
  return baseEvent({
    type: "item_delta",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    delta: {
      type: "context_compaction",
      stage,
      outcome,
      metrics: null,
      message,
    },
  }, sequence);
}

export function turnMetrics(
  sequence: number,
  overrides: Partial<{
    model_rounds: number;
    tool_calls: number;
    tool_successes: number;
    tool_errors: number;
    total_tokens: number;
  }> = {},
): TimelineEventEnvelope {
  return baseEvent({
    type: "turn_metrics",
    request_id: "req-1",
    turn_id: "turn-1",
    metrics: {
      model_rounds: overrides.model_rounds ?? 0,
      tool_calls: overrides.tool_calls ?? 0,
      tool_successes: overrides.tool_successes ?? 0,
      tool_errors: overrides.tool_errors ?? 0,
      stop_reason: "end_turn",
      usage: {
        ...usage(),
        total_tokens: overrides.total_tokens ?? 0,
      },
      duration_ms: null,
    },
  }, sequence);
}

export function turnCompleted(sequence: number, totalTokens = 0): TimelineEventEnvelope {
  return baseEvent({
    type: "turn_completed",
    request_id: "req-1",
    turn_id: "turn-1",
    completed_at_ms: sequence,
    stop_reason: "end_turn",
    usage: { ...usage(), total_tokens: totalTokens },
    metrics: executionMetrics(totalTokens),
  }, sequence);
}

export function sessionHistory(
  items: TalosSessionHistorySnapshot["messages"][number]["items"],
  metrics: TalosSessionHistorySnapshot["messages"][number]["metrics"] = null,
): TalosSessionHistorySnapshot {
  return {
    session_id: "sess-1",
    messages: [
      {
        message_id: "msg-1",
        turn_id: "turn-1",
        items,
        stop_reason: "end_turn",
        usage: usage(),
        metrics,
      },
    ],
    generated_at_ms: 1,
    last_stable_event_id: "ev_12",
  };
}

export function historyTextItem(
  itemId: string,
  kind: "assistant_text" | "user_input",
  phase: "commentary" | "final_answer" | "unknown" | "input",
  text: string,
): TalosSessionHistorySnapshot["messages"][number]["items"][number] {
  return {
    item_id: itemId,
    parent_item_id: null,
    kind,
    phase,
    status: "completed",
    title: null,
    started_at_ms: 1,
    completed_at_ms: 2,
    content: { type: "text", text },
  };
}

export function historyToolItem(
  itemId: string,
  output: string,
): TalosSessionHistorySnapshot["messages"][number]["items"][number] {
  return {
    item_id: itemId,
    parent_item_id: null,
    kind: "tool_call",
    phase: "acting",
    status: "completed",
    title: "Shell",
    started_at_ms: 1,
    completed_at_ms: 2,
    content: {
      type: "tool_call",
      call_id: "call-1",
      name: "Shell",
      arguments: "{}",
      progress: [],
      result: {
        content: [{ type: "text", text: output }],
        is_error: false,
      },
    },
  };
}

export function executionMetrics(totalTokens = 0) {
  return {
    model_rounds: 1,
    tool_calls: 0,
    tool_successes: 0,
    tool_errors: 0,
    stop_reason: "end_turn" as const,
    usage: { ...usage(), total_tokens: totalTokens },
    duration_ms: null,
  };
}

export function usage() {
  return {
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
    cache_creation_input_tokens: null,
    cache_read_input_tokens: null,
  };
}

export function activeTurn(overrides: Partial<TalosActiveTurn>): TalosActiveTurn {
  return {
    session_id: "sess-1",
    turn_id: "turn-1",
    message_id: "msg-1",
    status: "running",
    last_event_id: null,
    pending_user_action: null,
    updated_at: 1,
    ...overrides,
  };
}
