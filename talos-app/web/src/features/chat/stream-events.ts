import type {
  TalosActiveTurn,
  TalosTimelineContent,
  TalosTimelineDelta,
  TalosTimelineItem,
  TimelineEventEnvelope,
  TalosHistoryTimelineItem,
  TalosSessionHistorySnapshot,
} from "@talos/sdk-core";

import { useAppStore } from "@/shared/store/app-store";
import { outputContentToText } from "@/shared/types/domain";
import type { ChatRenderBlock, ChatTimelineItem } from "@/shared/types/ui";

type TimelineItemSnapshot = {
  item: TalosTimelineItem;
  callId?: string;
};

const timelineItemsBySession = new Map<string, Map<string, TimelineItemSnapshot>>();

export function buildBlocksFromHistorySnapshot(
  snapshot: TalosSessionHistorySnapshot | undefined,
): ChatTimelineBuildResult {
  const blocks: ChatRenderBlock[] = [];
  const inputItems: ChatTimelineBuildResult["inputItems"] = [];
  const messageIds = new Set<string>();
  let inputIndex = 0;

  function ensureTimelineBlock(messageId: string) {
    const id = `timeline-${snapshot?.session_id ?? "history"}-${messageId}`;
    const existing = blocks.find((block) => block.id === id);
    if (existing?.kind === "timeline") {
      return existing;
    }
    const block: ChatRenderBlock = {
      id,
      kind: "timeline",
      items: [],
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
      if (chatItem.kind !== "tool_call" && !chatItem.text.trim()) {
        continue;
      }
      const timeline = ensureTimelineBlock(message.message_id);
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

  if (
    isSameMessage &&
    stream?.lastEventSeq !== undefined &&
    envelope.event_seq <= stream.lastEventSeq
  ) {
    return;
  }

  if (stream?.activeMessageId && !isSameMessage) {
    state.resetLiveResponse(sessionId);
    timelineItemsBySession.delete(sessionId);
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
      rememberTimelineItem(sessionId, event.payload.item);
      applyItemStarted(sessionId, event.message_id, event.payload.item);
      return;
    case "item_delta":
      applyItemDelta(sessionId, event.payload.item_id, event.payload.delta, event.sequence);
      return;
    case "item_completed":
      applyItemCompleted(
        sessionId,
        event.payload.item_id,
        event.payload.status,
        Number(event.payload.completed_at_ms),
        event.payload.final_content,
      );
      return;
    case "turn_completed":
      markPendingInputCompleted(sessionId, event.message_id);
      timelineItemsBySession.delete(sessionId);
      state.setSessionStatus(
        sessionId,
        stream?.pendingApproval ? "waiting_approval" : stream?.pendingUserAction ? "running" : undefined,
      );
      state.setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        awaitingFinalResponse: false,
      });
      return;
    case "turn_error":
      timelineItemsBySession.delete(sessionId);
      state.setPendingUserAction(sessionId, undefined);
      state.setPendingApproval(sessionId, undefined);
      state.setSessionStatus(sessionId, "error");
      state.setStreamError(sessionId, event.payload.error || "Unknown stream error.");
      state.setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
      });
      return;
  }
}

function applyItemStarted(sessionId: string, messageId: string, item: TalosTimelineItem) {
  if (item.phase === "input") {
    const text = timelineText(item.content);
    if (text.trim()) {
      useAppStore.getState().upsertUserInputBlock(sessionId, messageId, text);
    }
    return;
  }
  if (item.phase === "final_answer") {
    return;
  }
  useAppStore.getState().upsertTimelineItem(sessionId, toChatTimelineItem(item));
}

function applyItemDelta(
  sessionId: string,
  itemId: string,
  delta: TalosTimelineDelta,
  sequence: number,
) {
  const item = getTimelineItem(sessionId, itemId);
  const state = useAppStore.getState();

  switch (delta.type) {
    case "text":
      if (!item) {
        return;
      }
      if (item.item.phase === "final_answer") {
        state.appendStreamText(sessionId, "assistant", delta.text);
      } else {
        state.appendTimelineItemText(sessionId, toChatTimelineItem(item.item), delta.text, sequence);
      }
      return;
    case "tool_arguments":
      if (item?.callId) {
        state.updateTimelineTool(sessionId, itemId, {
          arguments: appendToolArguments(item, delta.arguments_delta),
        });
      }
      return;
    case "tool_progress":
      if (item?.callId) {
        appendToolProgressSnapshot(item, delta.progress_kind, delta.text_delta);
        const progress = toolProgress(item);
        state.updateTimelineTool(sessionId, itemId, {
          progress,
        });
      }
      return;
    case "tool_result":
      if (item?.callId) {
        state.updateTimelineTool(sessionId, itemId, {
          output: outputContentToText(delta.content as never[]),
          isError: delta.is_error,
          status: delta.is_error ? "failed" : "completed",
          completedAtMs: Date.now(),
        });
        state.setStreamMeta(sessionId, { awaitingFinalResponse: true });
      }
      return;
    case "tool_approval_request":
      state.setPendingUserAction(sessionId, {
        type: "tool_approval",
        call_id: delta.call_id,
        tool_name: delta.tool_name,
        prompt: delta.prompt,
        arguments: unwrapJsonValue(delta.arguments),
      });
      ensureTimelineToolForRequest(sessionId, itemId, item, delta.call_id, delta.tool_name);
      state.updateTimelineTool(sessionId, itemId, {
        toolName: delta.tool_name,
        arguments: unwrapJsonValue(delta.arguments),
        prompt: delta.prompt,
        status: "waiting_approval",
      });
      state.setPendingApproval(sessionId, {
        callId: delta.call_id,
        toolName: delta.tool_name,
        prompt: delta.prompt,
        arguments: unwrapJsonValue(delta.arguments),
      });
      state.setSessionStatus(sessionId, "waiting_approval");
      return;
    case "tool_dynamic_request":
      state.setPendingUserAction(sessionId, {
        type: "dynamic_tool_result",
        call_id: delta.call_id,
        tool_name: delta.tool_name,
      });
      ensureTimelineToolForRequest(sessionId, itemId, item, delta.call_id, delta.tool_name);
      state.updateTimelineTool(sessionId, itemId, {
        toolName: delta.tool_name,
        arguments: unwrapJsonValue(delta.arguments),
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
        state.updateTimelineTool(sessionId, itemId, { status: delta.status });
      }
      return;
  }
}

function applyItemCompleted(
  sessionId: string,
  itemId: string,
  status: string,
  completedAtMs: number,
  finalContent: TalosTimelineContent | null,
) {
  const item = getTimelineItem(sessionId, itemId);
  if (item?.item.phase === "input") {
    markPendingInputCompleted(sessionId);
    return;
  }
  useAppStore.getState().completeTimelineItem(sessionId, itemId, status as ChatTimelineItem["status"], completedAtMs);
  if (!item?.callId || item.item.kind !== "tool_call") {
    return;
  }
  const content = finalContent?.type === "tool_call" ? finalContent : undefined;
  const result = content?.result;
  if (!result) {
    return;
  }
  useAppStore.getState().updateTimelineTool(sessionId, itemId, {
    output: outputContentToText(result.content as never[]),
    isError: result.is_error,
    status: result.is_error ? "failed" : "completed",
    completedAtMs,
  });
}

function rememberTimelineItem(sessionId: string, item: TalosTimelineItem) {
  let items = timelineItemsBySession.get(sessionId);
  if (!items) {
    items = new Map();
    timelineItemsBySession.set(sessionId, items);
  }
  items.set(item.item_id, {
    item,
    callId: item.content.type === "tool_call" ? item.content.call_id : undefined,
  });
}

function getTimelineItem(sessionId: string, itemId: string) {
  return timelineItemsBySession.get(sessionId)?.get(itemId);
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
    timestamp_ms: BigInt(Date.now()),
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
      output: item.content.result ? outputContentToText(item.content.result.content as never[]) : undefined,
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
  itemId: string,
  snapshot: TimelineItemSnapshot | undefined,
  callId: string,
  toolName: string,
) {
  if (snapshot) {
    return;
  }
  useAppStore.getState().upsertTimelineItem(sessionId, {
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

function unwrapJsonValue(value: { value: unknown }) {
  return value.value;
}

export function applyActiveTurnSnapshot(sessionId: string, activeTurn: TalosActiveTurn) {
  if (activeTurn.status === "completed") {
    const state = useAppStore.getState();
    const stream = state.streams[sessionId];
    markPendingInputCompleted(sessionId, activeTurn.message_id ?? undefined);
    state.setPendingUserAction(sessionId, undefined);
    state.setPendingApproval(sessionId, undefined);
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
    state.setPendingApproval(sessionId, undefined);
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

  if (activeTurn.status !== "waiting_user_action" || !pending) {
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

  if (pending.type === "tool_approval") {
    const itemId = pending.item_id ?? `pending-${pending.call_id}`;
    state.upsertTimelineItem(sessionId, {
      itemId,
      sourceItemId: itemId,
      kind: "tool_call",
      phase: "acting",
      status: "waiting_approval",
      title: pending.tool_name,
      callId: pending.call_id,
      toolName: pending.tool_name,
      arguments: pending.arguments,
      prompt: pending.prompt,
      progress: [],
      startedAtMs: Date.now(),
      completedAtMs: null,
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

  const itemId = pending.item_id ?? `pending-${pending.call_id}`;
  state.upsertTimelineItem(sessionId, {
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
  state.setPendingApproval(sessionId, undefined);
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
