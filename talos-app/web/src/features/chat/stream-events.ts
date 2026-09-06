import type {
  AgentAssistantTextPhase,
  AgentMessage,
  AgentMessageBlock,
  AgentMessageBlockContent,
  AgentMessageBlockDeltaContent,
  AgentRunMetrics,
  PendingUserAction,
  RecoverTimelineResponse,
  SessionHistoryResponse,
  TimelineActiveTurn,
  TimelineEvent,
} from "@talos/sdk-core";
import { LiveTurnProjection } from "./live-turn-projection";
import { decodeApiError, parseTimelineEvent } from "@talos/sdk-core";

import { useAppStore } from "@/shared/store/app-store";
import { outputContentToText } from "@/shared/types/domain";
import type {
  ChatRenderBlock,
  ChatTimelineItem,
  ChatTimelinePhase,
  ChatTurnMetrics,
  ChatUserInput,
} from "@/shared/types/ui";

const eventAccumulators = new Map<string, LiveTurnProjection>();

function eventAccumulator(sessionId: string) {
  let accumulator = eventAccumulators.get(sessionId);
  if (!accumulator) {
    accumulator = new LiveTurnProjection();
    eventAccumulators.set(sessionId, accumulator);
  }
  return accumulator;
}

function clearEventAccumulator(sessionId: string) {
  eventAccumulators.delete(sessionId);
}

export interface ChatTimelineBuildResult {
  blocks: ChatRenderBlock[];
  inputItems: Array<{
    messageId: string;
    itemIndex: number;
    text: string;
    fileIds: string[];
  }>;
  messageIds: Set<string>;
}

export function buildBlocksFromHistorySnapshot(
  response: SessionHistoryResponse | undefined,
): ChatTimelineBuildResult {
  const blocks: ChatRenderBlock[] = [];
  const inputItems: ChatTimelineBuildResult["inputItems"] = [];
  const messageIds = new Set<string>();
  let inputIndex = 0;

  for (const turn of response?.data ?? []) {
    const output = turn.result.result;
    const metrics = toChatTurnMetrics(
      output.metrics,
      turn.result.status === "completed" ? "completed" : "failed",
    );
    const toolItems = new Map<string, Extract<ChatTimelineItem, { kind: "tool_call" }>>();
    let segmentMessageId: string | undefined;
    let timeline: Extract<ChatRenderBlock, { kind: "timeline" }> | undefined;

    const ensureTimeline = () => {
      timeline ??= {
        id: segmentMessageId
          ? `timeline-${turn.identity.sessionId}-${turn.identity.turnId}-${segmentMessageId}`
          : `timeline-${turn.identity.sessionId}-${turn.identity.turnId}`,
        kind: "timeline",
        items: [],
        live: false,
      };
      return timeline;
    };
    const flushTimeline = () => {
      if (timeline?.items.length) {
        blocks.push(timeline);
      }
      timeline = undefined;
    };

    for (const message of output.messages) {
      messageIds.add(message.messageId);
      const input = message.role === "user" ? messageUserInput(message) : undefined;
      if (input && hasUserInput(input)) {
        flushTimeline();
        blocks.push({
          id: `user-${turn.identity.turnId}-${message.messageId}`,
          kind: "user",
          ...input,
          live: false,
        });
        inputItems.push({ messageId: message.messageId, itemIndex: inputIndex, ...input });
        inputIndex += 1;
        segmentMessageId = message.messageId;
      }

      for (const block of message.blocks) {
        if (block.content.type === "userContent") {
          continue;
        }
        if (block.content.type === "toolCall") {
          const item = blockToChatItem(block);
          if (item?.kind === "tool_call") {
            toolItems.set(block.content.data.callId, item);
            ensureTimeline().items.push(item);
          }
          continue;
        }
        if (block.content.type === "toolResult") {
          const tool = toolItems.get(block.content.data.callId);
          if (tool) {
            tool.output = outputContentToText(block.content.data.content);
            tool.contentItems = block.content.data.content;
            tool.isError = block.content.data.isError;
            tool.status = block.content.data.isError ? "failed" : "completed";
            tool.completedAtMs = block.completedAt === null ? null : block.completedAt * 1000;
          }
          continue;
        }
        const item = blockToChatItem(block);
        if (item) {
          ensureTimeline().items.push(item);
        }
      }
    }

    if (turn.result.status === "failed") {
      ensureTimeline().error = decodeApiError(turn.result.result.failure.error);
    }
    if (timeline) {
      timeline.metrics = metrics;
      if (timeline.error || timeline.items.length > 0 || hasMetrics(metrics)) {
        blocks.push(timeline);
      }
    } else if (hasMetrics(metrics)) {
      blocks.push({
        id: segmentMessageId
          ? `timeline-${turn.identity.sessionId}-${turn.identity.turnId}-${segmentMessageId}`
          : `timeline-${turn.identity.sessionId}-${turn.identity.turnId}`,
        kind: "timeline",
        items: [],
        metrics,
        live: false,
      });
    }
    for (const envelope of turn.notices) {
      const { event } = parseTimelineEvent(envelope);
      if (event.type !== "systemNotice") throw new Error("Invalid history notice.");
      blocks.push({ id: `notice-${envelope.eventId}`, kind: "notice", notice: event.data.notice, live: false });
    }
  }

  return { blocks, inputItems, messageIds };
}

export function applyStreamEvent(sessionId: string, envelope: TimelineEvent) {
  const { event: payload } = parseTimelineEvent(envelope);
  if (payload.data.identity.sessionId !== sessionId) {
    return;
  }

  const state = useAppStore.getState();
  const stream = state.streams[sessionId];
  if (
    stream?.lastEventSeq !== undefined &&
    envelope.eventSeq <= stream.lastEventSeq
  ) {
    return;
  }
  if (payload.type === "systemNotice") {
    state.upsertNotice(sessionId, envelope.eventId, payload.data.notice);
    state.setStreamMeta(sessionId, { lastEventId: envelope.eventId, lastEventSeq: envelope.eventSeq });
    return;
  }
  const event = payload.data;
  const sameTurn = stream?.activeTurnId === event.identity.turnId;
  if (!sameTurn) {
    if (stream?.activeTurnId) {
      state.resetLiveResponse(sessionId);
    }
    clearEventAccumulator(sessionId);
  }

  const accumulator = eventAccumulator(sessionId);
  if (!accumulator.apply(event)) {
    return;
  }

  state.setStreamMeta(sessionId, {
    activeTurnId: event.identity.turnId,
    lastEventId: envelope.eventId,
    lastEventSeq: envelope.eventSeq,
    lastSequence: event.sequence,
    isStreaming: true,
    connectionState: "streaming",
    reconnectAttempt: undefined,
    showReconnectControls: false,
  });

  switch (event.payload.type) {
    case "turnStarted":
      state.setStreamError(sessionId, undefined);
      state.setSessionStatus(sessionId, "running");
      return;
    case "messageStarted":
      applyMessageStarted(
        sessionId,
        event.identity.turnId,
        accumulator.message(event.payload.data.message.messageId) ?? event.payload.data.message,
      );
      return;
    case "messageBlockStarted":
      applyBlockStarted(
        sessionId,
        event.identity.turnId,
        accumulator.block(event.payload.data.messageId, event.payload.data.block.blockId) ??
          event.payload.data.block,
      );
      return;
    case "messageBlockDelta":
      applyBlockDelta(
        sessionId,
        accumulator,
        event.identity.turnId,
        event.payload.data.messageId,
        event.payload.data.blockId,
        event.payload.data.delta,
        event.sequence,
      );
      return;
    case "messageBlockCompleted":
      applyBlockCompleted(
        sessionId,
        accumulator,
        event.identity.turnId,
        event.payload.data.block,
      );
      return;
    case "messageCompleted":
      applyMessageCompleted(
        sessionId,
        event.identity.turnId,
        accumulator.message(event.payload.data.messageId) ?? event.payload.data,
        accumulator,
      );
      return;
    case "waitingForInput":
      if (accumulator.pendingInput()?.type === "userAction") {
        const pending = accumulator.pendingInput();
        if (!pending || pending.type !== "userAction") {
          return;
        }
        applyWaitingForUserAction(
          sessionId,
          event.identity.turnId,
          pending.action,
          accumulator,
        );
      }
      return;
    case "metricsUpdated":
      state.setTurnMetrics(
        sessionId,
        event.identity.turnId,
        toChatTurnMetrics(accumulator.metrics() ?? event.payload.data, "running"),
      );
      return;
    case "turnCompleted":
      markPendingInputCompleted(sessionId);
      completeTurnMetrics(sessionId, event.identity.turnId, "completed");
      state.setPendingUserAction(sessionId, undefined);
      state.setTimelineLive(sessionId, false);
      state.setSessionStatus(sessionId, undefined);
      state.setStreamMeta(sessionId, { isStreaming: false, connectionState: "idle" });
      clearEventAccumulator(sessionId);
      return;
    case "turnFailed":
      completeTurnMetrics(sessionId, event.identity.turnId, "failed");
      state.setTimelineLive(sessionId, false);
      state.setPendingUserAction(sessionId, undefined);
      state.setSessionStatus(sessionId, "error");
      state.setStreamError(sessionId, event.payload.data.error);
      state.setStreamMeta(sessionId, { isStreaming: false, connectionState: "idle" });
      clearEventAccumulator(sessionId);
      return;
  }
}

export function applyTimelineRecovery(
  sessionId: string,
  recovery: RecoverTimelineResponse,
) {
  if (recovery.activeTurn) {
    applyActiveTurnSnapshot(sessionId, recovery.activeTurn);
  }
  recovery.replay.events
    .slice()
    .sort((left, right) => left.eventSeq - right.eventSeq)
    .forEach((event) => applyStreamEvent(sessionId, event));
}

export function applyActiveTurnSnapshot(sessionId: string, activeTurn: TimelineActiveTurn) {
  const state = useAppStore.getState();
  const stream = state.streams[sessionId];
  const turnChanged = stream?.activeTurnId !== activeTurn.identity.turnId;
  if (turnChanged) {
    if (stream?.activeTurnId) {
      state.resetLiveResponse(sessionId);
    }
    clearEventAccumulator(sessionId);
  }

  state.setSessionStatus(sessionId, "running");
  state.setTimelineLive(sessionId, true);
  state.setStreamMeta(sessionId, {
    activeTurnId: activeTurn.identity.turnId,
    activeMessageId: activeTurn.initialMessageId ?? undefined,
    lastEventSeq: stream?.lastEventSeq,
    lastSequence: turnChanged ? undefined : stream?.lastSequence,
    isStreaming: true,
    connectionState: activeTurn.state === "cancelling" ? "cancelling" : "detached",
    reconnectAttempt: 0,
    showReconnectControls: false,
  });
}

export function clearInactiveTurnSnapshot(sessionId: string) {
  const state = useAppStore.getState();
  state.setPendingUserAction(sessionId, undefined);
  state.setSessionStatus(sessionId, undefined);
  state.setTimelineLive(sessionId, false);
  state.setStreamMeta(sessionId, {
    activeTurnId: undefined,
    activeMessageId: undefined,
    isStreaming: false,
    connectionState: "idle",
    reconnectAttempt: undefined,
    showReconnectControls: undefined,
  });
  clearEventAccumulator(sessionId);
}

function applyMessageStarted(sessionId: string, turnId: string, message: AgentMessage) {
  const state = useAppStore.getState();
  if (message.role !== "user") {
    return;
  }
  const input = messageUserInput(message);
  if (hasUserInput(input)) {
    state.upsertUserInputBlock(sessionId, turnId, message.messageId, input);
  }
  const pending = state.streams[sessionId]?.pendingInput;
  if (hasUserInput(input) && pending?.serverMessageId === message.messageId) {
    state.setPendingInput(sessionId, {
      ...pending,
      serverMessageId: message.messageId,
      phase: "completed",
    });
  }
  state.setPendingUserAction(sessionId, undefined);
  state.setStreamMeta(sessionId, {
    activeTurnId: turnId,
    activeMessageId: state.streams[sessionId]?.activeMessageId ?? message.messageId,
  });
}

function applyBlockStarted(
  sessionId: string,
  turnId: string,
  block: AgentMessageBlock,
) {
  const item = blockToChatItem(block);
  if (item) {
    useAppStore.getState().upsertTimelineItem(sessionId, turnId, item);
  }
}

function applyBlockDelta(
  sessionId: string,
  accumulator: LiveTurnProjection,
  turnId: string,
  messageId: string,
  blockId: string,
  delta: AgentMessageBlockDeltaContent,
  sequence: number,
) {
  const block = accumulator.block(messageId, blockId);
  if (!block) {
    return;
  }
  const state = useAppStore.getState();
  switch (delta.type) {
    case "text": {
      const item = blockToChatItem(block);
      if (item) {
        state.appendTimelineItemText(sessionId, turnId, item, delta.data, sequence);
      }
      return;
    }
    case "assistantTextPhase":
      if (block.content.type === "assistantText") {
        state.commitTimelineAssistantText(
          sessionId,
          turnId,
          blockId,
          assistantPhase(delta.data),
        );
      }
      return;
    case "toolCallArguments":
      if (block.content.type === "toolCall") {
        const args = block.content.data.arguments;
        if (args.state === "streaming") {
          state.updateTimelineTool(sessionId, turnId, blockId, {
            arguments: args.jsonFragment,
          });
        }
      }
      return;
    case "toolProgress":
      if (block.content.type === "toolCall") {
        state.updateTimelineTool(sessionId, turnId, blockId, {
          progress: block.content.data.progress.map((entry) => ({
            kind: entry.kind,
            message: entry.text,
          })),
        });
      }
      return;
    case "contextCompaction":
      if (block.content.type === "contextCompaction") {
        const item = blockToChatItem(block);
        if (item) {
          state.upsertTimelineItem(sessionId, turnId, item);
        }
      }
      return;
  }
}

function applyBlockCompleted(
  sessionId: string,
  accumulator: LiveTurnProjection,
  turnId: string,
  block: AgentMessageBlock,
) {
  if (block.content.type === "toolResult") {
    const toolRef = accumulator.toolCall(block.content.data.callId);
    if (!toolRef) {
      return;
    }
    useAppStore.getState().updateTimelineTool(sessionId, turnId, toolRef.block.blockId, {
      output: outputContentToText(block.content.data.content),
      contentItems: block.content.data.content,
      isError: block.content.data.isError,
      status: block.content.data.isError ? "failed" : "completed",
      completedAtMs: block.completedAt === null ? null : block.completedAt * 1000,
    });
    return;
  }
  const item = blockToChatItem(block);
  if (item) {
    useAppStore.getState().upsertTimelineItem(sessionId, turnId, item);
  }
}

function applyMessageCompleted(
  sessionId: string,
  turnId: string,
  message: AgentMessage,
  accumulator: LiveTurnProjection,
) {
  if (message.role === "user") {
    applyMessageStarted(sessionId, turnId, message);
    return;
  }
  const state = useAppStore.getState();
  for (const block of message.blocks) {
    if (block.content.type === "toolResult") {
      applyBlockCompleted(sessionId, accumulator, turnId, block);
      continue;
    }
    const item = blockToChatItem(block);
    if (item) {
      state.replaceTimelineItem(sessionId, turnId, item);
    }
  }
}

function applyWaitingForUserAction(
  sessionId: string,
  turnId: string,
  action: PendingUserAction,
  accumulator: LiveTurnProjection,
) {
  const state = useAppStore.getState();
  state.setPendingUserAction(sessionId, action);
  state.setSessionStatus(sessionId, "running");
  state.setTimelineLive(sessionId, true);
  state.setStreamMeta(sessionId, { isStreaming: true, connectionState: "detached" });
  const itemId = action.itemId;
  if (!itemId) {
    return;
  }
  const toolRef = accumulator.toolCall(action.callId);
  if (toolRef) {
    state.updateTimelineTool(sessionId, turnId, toolRef.block.blockId, {
      status: "waitingForInput",
      prompt: userActionPrompt(action),
    });
    return;
  }
  state.upsertTimelineItem(sessionId, turnId, {
    itemId,
    sourceItemId: itemId,
    kind: "tool_call",
    phase: "acting",
    status: "waitingForInput",
    title: userActionToolName(action),
    callId: action.callId,
    toolName: userActionToolName(action),
    arguments: null,
    progress: [{ kind: "status", message: "Waiting for user input." }],
    prompt: userActionPrompt(action),
    startedAtMs: Date.now(),
    completedAtMs: null,
  });
}

function blockToChatItem(block: AgentMessageBlock): ChatTimelineItem | undefined {
  const base = {
    itemId: block.blockId,
    sourceItemId: block.blockId,
    status: block.status,
    startedAtMs: block.startedAt * 1000,
    completedAtMs: block.completedAt === null ? null : block.completedAt * 1000,
  };
  switch (block.content.type) {
    case "userContent":
      return undefined;
    case "reasoning":
      return {
        ...base,
        kind: "reasoning",
        phase: "thinking",
        text: block.content.data.text,
      };
    case "assistantText":
      return {
        ...base,
        kind: "assistant_text",
        phase: assistantPhase(block.content.data.phase),
        text: block.content.data.text,
      };
    case "toolCall":
      return {
        ...base,
        kind: "tool_call",
        phase: "acting",
        title: block.content.data.name,
        callId: block.content.data.callId,
        toolName: block.content.data.name,
        arguments: toolArguments(block.content.data.arguments),
        progress: block.content.data.progress.map((entry) => ({
          kind: entry.kind,
          message: entry.text,
        })),
      };
    case "toolResult":
      return undefined;
    case "executionSummary":
      return {
        ...base,
        kind: "execution_summary",
        phase: "system",
        text: block.content.data.text,
      };
    case "contextCompaction":
      return {
        ...base,
        kind: "context_compaction",
        phase: "system",
        text: contextCompactionText(block.content.data),
        trigger: block.content.data.trigger,
        stage: block.content.data.stage,
        outcome: block.content.data.outcome,
        metrics: block.content.data.metrics,
      };
    case "systemNotice":
      return {
        ...base,
        kind: "system_notice",
        phase: "system",
        text: block.content.data.content.type === "text" ? block.content.data.content.data : "",
        notice: block.content.data,
      };
  }
}

function assistantPhase(phase: AgentAssistantTextPhase): ChatTimelinePhase {
  switch (phase) {
    case "pending":
      return "pending";
    case "commentary":
      return "commentary";
    case "finalAnswer":
      return "final_answer";
  }
}

function toolArguments(
  args: Extract<AgentMessageBlockContent, { type: "toolCall" }>["data"]["arguments"],
) {
  return args.state === "streaming" ? args.jsonFragment : args.value;
}

function messageUserInput(message: AgentMessage): ChatUserInput {
  const text: string[] = [];
  const fileIds: string[] = [];
  for (const block of message.blocks) {
    if (block.content.type !== "userContent") {
      continue;
    }
    const content = block.content.data;
    if (content.type === "text") {
      text.push(content.text);
    } else if (content.type === "file") {
      fileIds.push(content.fileId);
    }
  }
  return { text: text.join("\n"), fileIds };
}

function hasUserInput(input: ChatUserInput) {
  return Boolean(input.text.trim() || input.fileIds.length > 0);
}

function toChatTurnMetrics(
  metrics: AgentRunMetrics,
  state: ChatTurnMetrics["state"],
): ChatTurnMetrics {
  return { ...metrics, state };
}

function hasMetrics(metrics: ChatTurnMetrics) {
  return (
    metrics.state !== "running" ||
    metrics.modelRounds > 0 ||
    metrics.toolCalls > 0 ||
    metrics.toolSuccesses > 0 ||
    metrics.toolErrors > 0 ||
    metrics.usage.total_tokens > 0 ||
    metrics.durationMs != null
  );
}

function contextCompactionText(
  content: Extract<AgentMessageBlockContent, { type: "contextCompaction" }>["data"],
) {
  const message = content.message?.trim();
  if (message) {
    return message;
  }
  const stage = formatProtocolLabel(content.stage);
  const outcome = content.outcome ? ` (${formatProtocolLabel(content.outcome)})` : "";
  return `Context compaction: ${stage}${outcome}`;
}

function formatProtocolLabel(value: string) {
  return value
    .split("_")
    .filter(Boolean)
    .map((part) => `${part.charAt(0).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}

function userActionToolName(action: PendingUserAction) {
  switch (action.params.type) {
    case "dynamicToolResult":
      return action.params.params.toolName;
    case "question":
      return "AskUserQuestion";
    case "sandboxApproval":
      return action.params.params.toolName;
  }
}

function userActionPrompt(action: PendingUserAction) {
  switch (action.params.type) {
    case "dynamicToolResult":
      return undefined;
    case "question":
      return action.params.params.question;
    case "sandboxApproval":
      return action.params.params.reason;
  }
}

function markPendingInputCompleted(sessionId: string) {
  const state = useAppStore.getState();
  const pending = state.streams[sessionId]?.pendingInput;
  if (!pending) {
    return;
  }
  state.setPendingInput(sessionId, { ...pending, phase: "completed" });
}

function completeTurnMetrics(
  sessionId: string,
  turnId: string,
  status: ChatTurnMetrics["state"],
) {
  const state = useAppStore.getState();
  const metrics = state.streams[sessionId]?.turnMetrics;
  if (metrics) {
    state.setTurnMetrics(sessionId, turnId, { ...metrics, state: status });
  }
}
