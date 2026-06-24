import {
  dynamicToolArgumentsValue,
  type TalosActiveTurn,
  type TalosTimelineContent,
  type TalosTimelineDelta,
  type TalosTimelineItem,
  type TalosTurnExecutionMetrics,
  type TimelineEventEnvelope,
  type TalosHistoryTimelineItem,
  type TalosSessionHistorySnapshot,
} from "@talos/sdk-core";

import { useAppStore } from "@/shared/store/app-store";
import { outputContentToText } from "@/shared/types/domain";
import type { ChatRenderBlock, ChatTimelineItem, ChatTurnMetrics } from "@/shared/types/ui";

type TimelineItemSnapshot = {
  item: TalosTimelineItem;
  callId?: string;
};

const timelineItemsByMessage = new Map<string, Map<string, TimelineItemSnapshot>>();

function timelineMessageKey(sessionId: string, messageId: string) {
  return `${sessionId}:${messageId}`;
}

function clearTimelineItemsForSession(sessionId: string) {
  for (const key of timelineItemsByMessage.keys()) {
    if (key.startsWith(`${sessionId}:`)) {
      timelineItemsByMessage.delete(key);
    }
  }
}

export function buildBlocksFromHistorySnapshot(
  snapshot: TalosSessionHistorySnapshot | undefined,
): ChatTimelineBuildResult {
  const blocks: ChatRenderBlock[] = [];
  const inputItems: ChatTimelineBuildResult["inputItems"] = [];
  const messageIds = new Set<string>();
  let inputIndex = 0;

  function ensureTimelineBlock(messageId: string, metrics?: ChatTurnMetrics) {
    const id = `timeline-${snapshot?.session_id ?? "history"}-${messageId}`;
    const existing = blocks.find((block) => block.id === id);
    if (existing?.kind === "timeline") {
      existing.metrics = metrics ?? existing.metrics;
      return existing;
    }
    const block: ChatRenderBlock = {
      id,
      kind: "timeline",
      items: [],
      metrics,
      live: false,
    };
    blocks.push(block);
    return block;
  }

  function appendAssistantText(messageId: string, text: string) {
    if (!text.trim()) {
      return;
    }
    const id = `assistant-${messageId}`;
    const existing = blocks.find((block) => block.id === id);
    if (existing?.kind === "assistant") {
      existing.text += text;
      return;
    }
    blocks.push({
      id,
      kind: "assistant",
      text,
      live: false,
    });
  }

  for (const message of snapshot?.messages ?? []) {
    messageIds.add(message.message_id);
    const messageMetrics = message.metrics
      ? toChatTurnMetrics(message.metrics, "completed")
      : undefined;
    for (const item of message.items) {
      if (item.phase === "input") {
        const text = timelineText(item.content);
        if (text.trim()) {
          blocks.push({
            id: `user-${message.message_id}`,
            kind: "user",
            text,
            live: false,
          });
          inputItems.push({ messageId: message.message_id, itemIndex: inputIndex, text });
          inputIndex += 1;
        }
        continue;
      }
      if (item.phase === "final_answer") {
        appendAssistantText(message.message_id, timelineText(item.content));
        continue;
      }
      const chatItem = toChatTimelineItem(item);
      const timeline = ensureTimelineBlock(message.message_id, messageMetrics);
      const existingIndex = timeline.items.findIndex((entry) => entry.itemId === chatItem.itemId);
      if (existingIndex >= 0) {
        timeline.items[existingIndex] = mergeTimelineHistoryItem(
          timeline.items[existingIndex],
          chatItem,
        );
      } else {
        timeline.items.push(chatItem);
      }
    }
    if (messageMetrics) {
      ensureTimelineBlock(message.message_id, messageMetrics);
    }
  }

  return { blocks, inputItems, messageIds };
}

export interface ChatTimelineBuildResult {
  blocks: ChatRenderBlock[];
  inputItems: Array<{ messageId: string; itemIndex: number; text: string }>;
  messageIds: Set<string>;
}

function mergeTimelineHistoryItem(
  current: ChatTimelineItem,
  next: ChatTimelineItem,
): ChatTimelineItem {
  if (current.kind === "tool_call" && next.kind === "tool_call") {
    return {
      ...current,
      ...next,
      progress: next.progress.length > 0 ? next.progress : current.progress,
      output: next.output ?? current.output,
      prompt: next.prompt ?? current.prompt,
      completedAtMs: next.completedAtMs ?? current.completedAtMs,
    };
  }
  if (current.kind !== "tool_call" && next.kind !== "tool_call") {
    return {
      ...current,
      ...next,
      text: next.text || current.text,
      completedAtMs: next.completedAtMs ?? current.completedAtMs,
    };
  }
  return next;
}

export function applyStreamEvent(sessionId: string, envelope: TimelineEventEnvelope) {
  const event = envelope.event;
  const state = useAppStore.getState();
  const stream = state.streams[sessionId];
  const isSameMessage = stream?.activeMessageId === event.message_id;

  if (event.payload.type === "turn_metrics") {
    if (stream?.activeMessageId && !isSameMessage) {
      return;
    }
    state.setTurnMetrics(
      sessionId,
      event.message_id,
      toChatTurnMetrics(event.payload.metrics as TalosTurnExecutionMetrics, "running"),
    );
    return;
  }

  if (
    isSameMessage &&
    stream?.lastEventSeq !== undefined &&
    envelope.event_seq <= stream.lastEventSeq
  ) {
    return;
  }

  if (stream?.activeMessageId && !isSameMessage) {
    state.resetLiveResponse(sessionId);
    clearTimelineItemsForSession(sessionId);
  }

  if (stream?.pendingInput && stream.pendingInput.serverMessageId !== event.message_id) {
    state.setPendingInput(sessionId, {
      ...stream.pendingInput,
      serverMessageId: event.message_id,
      phase: "accepted",
    });
  } else if (stream?.pendingInput && stream.pendingInput.phase === "local") {
    state.setPendingInput(sessionId, {
      ...stream.pendingInput,
      phase: "accepted",
    });
  }

  state.setStreamMeta(sessionId, {
    lastEventId: envelope.event_id,
    lastEventSeq: envelope.event_seq,
    lastSequence: event.sequence,
    activeMessageId: event.message_id,
    isStreaming: true,
    connectionState: "streaming",
    reconnectAttempt: undefined,
    showReconnectControls: false,
    awaitingFinalResponse: false,
  });

  switch (event.payload.type) {
    case "turn_started":
      state.setSessionStatus(sessionId, "running");
      return;
    case "item_started":
      applyItemStarted(sessionId, event.message_id, event.payload.item);
      return;
    case "item_delta":
      applyItemDelta(sessionId, event.message_id, event.payload.item_id, event.payload.delta, event.sequence);
      return;
    case "item_completed":
      applyItemCompleted(
        sessionId,
        event.message_id,
        event.payload.item_id,
        event.payload.status,
        Number(event.payload.completed_at_ms),
        event.payload.final_content,
      );
      return;
    case "assistant_text_committed":
      applyAssistantTextCommitted(sessionId, event.message_id, event.payload.item_id, event.payload.phase);
      return;
    case "turn_completed":
      if (event.payload.metrics) {
        state.setTurnMetrics(
          sessionId,
          event.message_id,
          toChatTurnMetrics(event.payload.metrics as TalosTurnExecutionMetrics, "completed"),
        );
      }
      markPendingInputCompleted(sessionId, event.message_id);
      timelineItemsByMessage.delete(timelineMessageKey(sessionId, event.message_id));
      state.setSessionStatus(
        sessionId,
        stream?.pendingUserAction ? "running" : undefined,
      );
      state.setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        awaitingFinalResponse: false,
      });
      return;
    case "turn_error":
      timelineItemsByMessage.delete(timelineMessageKey(sessionId, event.message_id));
      state.setPendingUserAction(sessionId, undefined);
      state.setSessionStatus(sessionId, "error");
      state.setStreamError(sessionId, event.payload.error || "Unknown stream error.");
      state.setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
      });
      return;
  }
}

function toChatTurnMetrics(
  metrics: TalosTurnExecutionMetrics,
  state: ChatTurnMetrics["state"],
): ChatTurnMetrics {
  return {
    ...metrics,
    state,
    model_rounds: Number(metrics.model_rounds),
    tool_calls: Number(metrics.tool_calls),
    tool_successes: Number(metrics.tool_successes),
    tool_errors: Number(metrics.tool_errors),
    duration_ms: metrics.duration_ms == null ? null : Number(metrics.duration_ms),
    usage: {
      ...metrics.usage,
      input_tokens: Number(metrics.usage.input_tokens),
      output_tokens: Number(metrics.usage.output_tokens),
      total_tokens: Number(metrics.usage.total_tokens),
      cache_creation_input_tokens:
        metrics.usage.cache_creation_input_tokens == null
          ? metrics.usage.cache_creation_input_tokens
          : Number(metrics.usage.cache_creation_input_tokens),
      cache_read_input_tokens:
        metrics.usage.cache_read_input_tokens == null
          ? metrics.usage.cache_read_input_tokens
          : Number(metrics.usage.cache_read_input_tokens),
    },
  };
}

function applyItemStarted(sessionId: string, messageId: string, item: TalosTimelineItem) {
  rememberTimelineItem(sessionId, messageId, item);
  if (item.phase === "input") {
    const text = timelineText(item.content);
    if (text.trim()) {
      useAppStore.getState().upsertUserInputBlock(sessionId, messageId, text);
    }
    return;
  }
  if (item.phase === "final_answer") {
    useAppStore.getState().setTimelineLive(sessionId, false);
    return;
  }
  useAppStore.getState().upsertTimelineItem(sessionId, messageId, toChatTimelineItem(item));
}

function applyItemDelta(
  sessionId: string,
  messageId: string,
  itemId: string,
  delta: TalosTimelineDelta,
  sequence: number,
) {
  const item = getTimelineItem(sessionId, messageId, itemId);
  const state = useAppStore.getState();

  switch (delta.type) {
    case "text":
      if (!item) {
        return;
      }
      if (item.item.phase === "final_answer") {
        state.appendStreamText(sessionId, "assistant", delta.text);
      } else {
        state.appendTimelineItemText(sessionId, messageId, toChatTimelineItem(item.item), delta.text, sequence);
      }
      return;
    case "tool_arguments":
      if (item?.callId) {
        state.updateTimelineTool(sessionId, messageId, itemId, {
          arguments: appendToolArguments(item, delta.arguments_delta),
        });
      }
      return;
    case "tool_progress":
      if (item?.callId) {
        appendToolProgressSnapshot(item, delta.progress_kind, delta.text_delta);
        const progress = toolProgress(item);
        state.updateTimelineTool(sessionId, messageId, itemId, {
          progress,
        });
      }
      return;
    case "tool_result":
      if (item?.callId) {
        state.updateTimelineTool(sessionId, messageId, itemId, {
          output: outputContentToText(delta.content),
          contentItems: delta.content,
          isError: delta.is_error,
          status: delta.is_error ? "failed" : "completed",
          completedAtMs: Date.now(),
        });
        state.setStreamMeta(sessionId, { awaitingFinalResponse: true });
      }
      return;
    case "tool_dynamic_request":
      state.setPendingUserAction(sessionId, {
        type: "dynamic_tool_result",
        call_id: delta.call_id,
        tool_name: delta.tool_name,
      });
      ensureTimelineToolForRequest(sessionId, messageId, itemId, item, delta.call_id, delta.tool_name);
      state.updateTimelineTool(sessionId, messageId, itemId, {
        toolName: delta.tool_name,
        arguments: dynamicToolArgumentsValue(delta.arguments),
        status: "running",
        progress: [
          ...toolProgress(item),
          { kind: "status", message: "Waiting for dynamic tool result." },
        ],
      });
      state.setSessionStatus(sessionId, "running");
      return;
    case "status":
      if (item?.callId) {
        state.updateTimelineTool(sessionId, messageId, itemId, { status: delta.status });
      }
      return;
  }
}

function applyItemCompleted(
  sessionId: string,
  messageId: string,
  itemId: string,
  status: string,
  completedAtMs: number,
  finalContent: TalosTimelineContent | null,
) {
  const item = getTimelineItem(sessionId, messageId, itemId);
  if (item?.item.phase === "input") {
    markPendingInputCompleted(sessionId);
    return;
  }
  useAppStore.getState().completeTimelineItem(sessionId, messageId, itemId, status as ChatTimelineItem["status"], completedAtMs);
  if (!item?.callId || item.item.kind !== "tool_call") {
    return;
  }
  const content = finalContent?.type === "tool_call" ? finalContent : undefined;
  const result = content?.result;
  if (!result) {
    return;
  }
  useAppStore.getState().updateTimelineTool(sessionId, messageId, itemId, {
    output: outputContentToText(result.content),
    contentItems: result.content,
    isError: result.is_error,
    status: result.is_error ? "failed" : "completed",
    completedAtMs,
  });
}

function applyAssistantTextCommitted(
  sessionId: string,
  messageId: string,
  itemId: string,
  phase: TalosTimelineItem["phase"],
) {
  const item = getTimelineItem(sessionId, messageId, itemId);
  if (item) {
    item.item.phase = phase;
  }
  const state = useAppStore.getState();
  state.commitTimelineAssistantText(sessionId, messageId, itemId, phase);
  if (phase === "final_answer") {
    state.setTimelineLive(sessionId, false);
  }
}

function rememberTimelineItem(sessionId: string, messageId: string, item: TalosTimelineItem) {
  const key = timelineMessageKey(sessionId, messageId);
  let items = timelineItemsByMessage.get(key);
  if (!items) {
    items = new Map();
    timelineItemsByMessage.set(key, items);
  }
  items.set(item.item_id, {
    item,
    callId: item.content.type === "tool_call" ? item.content.call_id : undefined,
  });
}

function getTimelineItem(sessionId: string, messageId: string, itemId: string) {
  return timelineItemsByMessage.get(timelineMessageKey(sessionId, messageId))?.get(itemId);
}

function appendToolArguments(item: TimelineItemSnapshot, delta: string) {
  if (item.item.content.type !== "tool_call") {
    return delta;
  }
  item.item.content.arguments += delta;
  return item.item.content.arguments;
}

function appendToolProgressSnapshot(
  item: TimelineItemSnapshot,
  progressKind: "status" | "stdout" | "stderr",
  text: string,
) {
  if (item.item.content.type !== "tool_call") {
    return;
  }
  item.item.content.progress.push({
    progress_kind: progressKind,
    text,
    timestamp_ms: Date.now(),
  });
}

function toChatTimelineItem(item: TalosTimelineItem | TalosHistoryTimelineItem): ChatTimelineItem {
  const startedAtMs = Number(item.started_at_ms);
  const completedAtMs = item.completed_at_ms == null ? null : Number(item.completed_at_ms);
  if (item.kind === "tool_call" && item.content.type === "tool_call") {
    return {
      itemId: item.item_id,
      sourceItemId: item.item_id,
      kind: "tool_call",
      phase: item.phase,
      status: item.status,
      title: item.title,
      callId: item.content.call_id,
      toolName: item.content.name,
      arguments: item.content.arguments,
      output: item.content.result ? outputContentToText(item.content.result.content) : undefined,
      contentItems: item.content.result?.content,
      isError: item.content.result?.is_error,
      progress: item.content.progress.map((entry) => ({
        kind: entry.progress_kind,
        message: entry.text,
      })),
      startedAtMs,
      completedAtMs,
    };
  }
  return {
    itemId: item.item_id,
    sourceItemId: item.item_id,
    kind: item.kind === "tool_call" ? "system_notice" : item.kind,
    phase: item.phase,
    status: item.status,
    title: item.title,
    text: timelineText(item.content),
    startedAtMs,
    completedAtMs,
  };
}

function timelineText(content: TalosTimelineContent) {
  switch (content.type) {
    case "text":
      return content.text;
    case "execution_summary":
      return content.summary;
    case "system_notice":
      return content.message;
    case "empty":
    case "tool_call":
      return "";
  }
}

function ensureTimelineToolForRequest(
  sessionId: string,
  messageId: string,
  itemId: string,
  snapshot: TimelineItemSnapshot | undefined,
  callId: string,
  toolName: string,
) {
  if (snapshot) {
    return;
  }
  useAppStore.getState().upsertTimelineItem(sessionId, messageId, {
    itemId,
    kind: "tool_call",
    phase: "acting",
    status: "running",
    title: toolName,
    callId,
    toolName,
    arguments: null,
    progress: [],
    startedAtMs: Date.now(),
    completedAtMs: null,
  });
}

function toolProgress(item: TimelineItemSnapshot | undefined) {
  if (!item || item.item.content.type !== "tool_call") {
    return [];
  }
  return item.item.content.progress.map((entry) => ({
    kind: entry.progress_kind,
    message: entry.text,
  }));
}

export function applyActiveTurnSnapshot(sessionId: string, activeTurn: TalosActiveTurn) {
  if (activeTurn.status === "completed") {
    const state = useAppStore.getState();
    const stream = state.streams[sessionId];
    markPendingInputCompleted(sessionId, activeTurn.message_id ?? undefined);
    state.setPendingUserAction(sessionId, undefined);
    state.setSessionStatus(sessionId, undefined);
    state.setStreamMeta(sessionId, {
      activeMessageId: activeTurn.message_id ?? undefined,
      lastEventId: activeTurn.last_event_id ?? stream?.lastEventId,
      lastEventSeq: stream?.lastEventSeq,
      isStreaming: false,
      connectionState: "idle",
      reconnectAttempt: undefined,
      showReconnectControls: undefined,
      awaitingFinalResponse: false,
    });
    return;
  }

  if (activeTurn.status === "failed") {
    const state = useAppStore.getState();
    const stream = state.streams[sessionId];
    state.setPendingUserAction(sessionId, undefined);
    state.setSessionStatus(sessionId, "error");
    state.setStreamMeta(sessionId, {
      lastEventId: activeTurn.last_event_id ?? stream?.lastEventId,
      lastEventSeq: stream?.lastEventSeq,
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
      clearTimelineItemsForSession(sessionId);
    }
    if (stream?.pendingInput && stream.pendingInput.serverMessageId !== activeTurn.message_id) {
      state.setPendingInput(sessionId, {
        ...stream.pendingInput,
        serverMessageId: activeTurn.message_id,
        phase: "accepted",
      });
    } else if (stream?.pendingInput && stream.pendingInput.phase === "local") {
      state.setPendingInput(sessionId, {
        ...stream.pendingInput,
        phase: "accepted",
      });
    }
    state.setStreamMeta(sessionId, {
      activeMessageId: activeTurn.message_id,
      lastEventId: activeTurn.last_event_id ?? stream?.lastEventId,
      lastEventSeq: messageChanged ? undefined : stream?.lastEventSeq,
      lastSequence: messageChanged ? undefined : stream?.lastSequence,
      isStreaming: true,
      connectionState: "detached",
      reconnectAttempt: 0,
      showReconnectControls: false,
    });
    state.setSessionStatus(sessionId, "running");
    return;
  }

  if (activeTurn.status !== "waiting_user_action" || !pending || !activeTurn.message_id) {
    return;
  }

  const state = useAppStore.getState();
  state.setPendingUserAction(sessionId, pending);
  const stream = state.streams[sessionId];
  state.setStreamMeta(sessionId, {
    activeMessageId: activeTurn.message_id ?? undefined,
    lastEventId: activeTurn.last_event_id ?? stream?.lastEventId,
    lastEventSeq: stream?.lastEventSeq,
    isStreaming: true,
    connectionState: "detached",
  });

  const itemId = pending.item_id ?? `pending-${pending.call_id}`;
  state.upsertTimelineItem(sessionId, activeTurn.message_id, {
    itemId,
    sourceItemId: itemId,
    kind: "tool_call",
    phase: "acting",
    status: "running",
    title: pending.tool_name,
    callId: pending.call_id,
    toolName: pending.tool_name,
    arguments: null,
    progress: [{ kind: "status", message: "Waiting for dynamic tool result." }],
    startedAtMs: Date.now(),
    completedAtMs: null,
  });
  state.setSessionStatus(sessionId, "running");
}

export function clearInactiveTurnSnapshot(sessionId: string) {
  const state = useAppStore.getState();
  state.setPendingUserAction(sessionId, undefined);
  state.setSessionStatus(sessionId, undefined);
  state.setStreamMeta(sessionId, {
    isStreaming: false,
    connectionState: "idle",
    reconnectAttempt: undefined,
    showReconnectControls: undefined,
    awaitingFinalResponse: false,
  });
}

function markPendingInputCompleted(sessionId: string, messageId?: string) {
  const state = useAppStore.getState();
  const pending = state.streams[sessionId]?.pendingInput;
  if (!pending) {
    return;
  }
  if (messageId && pending.serverMessageId && pending.serverMessageId !== messageId) {
    return;
  }
  state.setPendingInput(sessionId, {
    ...pending,
    serverMessageId: messageId ?? pending.serverMessageId,
    phase: "completed",
  });
}
