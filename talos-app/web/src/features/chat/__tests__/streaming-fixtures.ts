import type {
  AgentAssistantTextPhase,
  AgentContextCompaction,
  AgentEventPayload,
  AgentMessage,
  AgentMessageBlock,
  AgentMessageBlockContent,
  AgentRunMetrics,
  AgentStopReason,
  ContextCompactionMetrics,
  ContextCompactionOutcome,
  ContextCompactionStage,
  ContextCompactionTrigger,
  PendingUserAction,
  SessionHistoryResponse,
  TalosContentItem,
  TimelineActiveTurn,
  TimelineEvent,
} from "@talos/sdk-core";

export function baseEvent(payload: AgentEventPayload, sequence: number): TimelineEvent {
  return {
    eventId: `ev_${sequence}`,
    eventSeq: sequence,
    persistedAt: sequence,
    event: {
      identity: { sessionId: "sess-1", turnId: "turn-1" },
      sequence,
      occurredAt: sequence,
      payload,
    },
  };
}

export function itemStarted(
  itemId: string,
  kind: "assistant_text" | "tool_call" | "user_input" | "reasoning",
  phase: "commentary" | "final_answer" | "pending" | "acting" | "input" | "thinking",
  sequence: number,
): TimelineEvent {
  const content: AgentMessageBlockContent = kind === "tool_call"
    ? {
        type: "toolCall",
        data: {
          callId: `call-${itemId}`,
          name: "Shell",
          arguments: { state: "streaming", jsonFragment: "{}" },
          progress: [],
        },
      }
    : kind === "reasoning"
      ? { type: "reasoning", data: { text: "" } }
      : kind === "user_input"
        ? { type: "userContent", data: { type: "text", text: "" } }
        : {
            type: "assistantText",
            data: { phase: assistantProtocolPhase(phase), text: "" },
          };
  return baseEvent({
    type: "messageBlockStarted",
    data: {
      messageId: "msg-1",
      block: block(itemId, content, "running", sequence, null),
    },
  }, sequence);
}

export function withMessage(event: TimelineEvent, messageId: string, turnId: string): TimelineEvent {
  const payload = event.event.payload;
  if (
    payload.type === "messageBlockStarted" ||
    payload.type === "messageBlockDelta" ||
    payload.type === "messageBlockCompleted"
  ) {
    payload.data.messageId = messageId;
  } else if (payload.type === "messageStarted") {
    payload.data.message.messageId = messageId;
    payload.data.message.turnId = turnId;
  }
  return {
    ...event,
    event: {
      ...event.event,
      identity: { ...event.event.identity, turnId },
      payload,
    },
  };
}

export function assistantTextCommitted(
  itemId: string,
  phase: "commentary" | "final_answer",
  sequence: number,
): TimelineEvent {
  return baseEvent({
    type: "messageBlockDelta",
    data: {
      messageId: "msg-1",
      blockId: itemId,
      delta: { type: "assistantTextPhase", data: assistantProtocolPhase(phase) },
    },
  }, sequence);
}

export function userInputStarted(
  itemId: string,
  text: string,
  sequence: number,
  fileIds: string[] = [],
  messageId = `msg-${itemId}`,
): TimelineEvent {
  const blocks: AgentMessageBlock[] = [
    block(itemId, { type: "userContent", data: { type: "text", text } }, "running", sequence, null),
    ...fileIds.map((fileId, index) =>
      block(
        `${itemId}-file-${index}`,
        { type: "userContent", data: { type: "file", fileId } },
        "running",
        sequence,
        null,
      )),
  ];
  return baseEvent({
    type: "messageStarted",
    data: {
      message: message(messageId, "turn-1", "user", blocks, "inProgress"),
    },
  }, sequence);
}

export function itemCompleted(
  itemId: string,
  sequence: number,
  kind: "assistant_text" | "tool_call" | "user_input" | "reasoning" = "assistant_text",
  phase: AgentAssistantTextPhase = "pending",
): TimelineEvent {
  const content: AgentMessageBlockContent = kind === "tool_call"
    ? {
        type: "toolCall",
        data: {
          callId: `call-${itemId}`,
          name: "Shell",
          arguments: { state: "complete", value: {} },
          progress: [],
        },
      }
    : kind === "reasoning"
      ? { type: "reasoning", data: { text: "" } }
      : kind === "user_input"
        ? { type: "userContent", data: { type: "text", text: "" } }
        : { type: "assistantText", data: { phase, text: "" } };
  return baseEvent({
    type: "messageBlockCompleted",
    data: {
      messageId: "msg-1",
      block: block(itemId, content, "completed", sequence, sequence),
    },
  }, sequence);
}

export function contextCompactionCompleted(
  itemId: string,
  sequence: number,
  outcome: ContextCompactionOutcome,
  messageText: string,
): TimelineEvent {
  return baseEvent({
    type: "messageBlockCompleted",
    data: {
      messageId: "msg-1",
      block: block(
        itemId,
        {
          type: "contextCompaction",
          data: { ...compaction("completed", messageText), outcome },
        },
        "completed",
        sequence,
        sequence,
      ),
    },
  }, sequence);
}

export function textDelta(itemId: string, text: string, sequence: number): TimelineEvent {
  return baseEvent({
    type: "messageBlockDelta",
    data: {
      messageId: "msg-1",
      blockId: itemId,
      delta: { type: "text", data: text },
    },
  }, sequence);
}

export function toolProgress(itemId: string, text: string, sequence: number): TimelineEvent {
  return baseEvent({
    type: "messageBlockDelta",
    data: {
      messageId: "msg-1",
      blockId: itemId,
      delta: {
        type: "toolProgress",
        data: { kind: "status", text, occurredAt: sequence },
      },
    },
  }, sequence);
}

export function toolResult(
  itemId: string,
  content: string | TalosContentItem[],
  sequence: number,
): TimelineEvent {
  const result = typeof content === "string" ? [{ type: "text" as const, text: content }] : content;
  return baseEvent({
    type: "messageBlockCompleted",
    data: {
      messageId: "tool-message-1",
      block: block(
        `${itemId}-result`,
        {
          type: "toolResult",
          data: {
            callId: `call-${itemId}`,
            toolName: "Shell",
            content: result,
            isError: false,
          },
        },
        "completed",
        sequence,
        sequence,
      ),
    },
  }, sequence);
}

export function contextCompactionStarted(
  itemId: string,
  stage: ContextCompactionStage,
  messageText: string | null,
  sequence: number,
  trigger: ContextCompactionTrigger = "threshold_exceeded",
): TimelineEvent {
  return baseEvent({
    type: "messageBlockStarted",
    data: {
      messageId: "msg-1",
      block: block(
        itemId,
        {
          type: "contextCompaction",
          data: { ...compaction(stage, messageText), trigger },
        },
        "running",
        sequence,
        null,
      ),
    },
  }, sequence);
}

export function contextCompactionDelta(
  itemId: string,
  stage: ContextCompactionStage,
  outcome: ContextCompactionOutcome | null,
  messageText: string | null,
  sequence: number,
  metrics: ContextCompactionMetrics | null = null,
): TimelineEvent {
  return baseEvent({
    type: "messageBlockDelta",
    data: {
      messageId: "msg-1",
      blockId: itemId,
      delta: {
        type: "contextCompaction",
        data: { stage, outcome, metrics, message: messageText },
      },
    },
  }, sequence);
}

export function turnMetrics(
  sequence: number,
  overrides: Partial<{
    modelRounds: number;
    toolCalls: number;
    toolSuccesses: number;
    toolErrors: number;
    total_tokens: number;
  }> = {},
): TimelineEvent {
  return baseEvent({
    type: "metricsUpdated",
    data: executionMetrics(overrides.total_tokens ?? 0, overrides),
  }, sequence);
}

export function turnCompleted(
  sequence: number,
  stopReason: AgentStopReason = "endTurn",
): TimelineEvent {
  return baseEvent({
    type: "turnCompleted",
    data: { stopReason },
  }, sequence);
}

export function waitingForUserAction(
  sequence: number,
  action: PendingUserAction = pendingUserAction(),
): TimelineEvent {
  return baseEvent({
    type: "waitingForInput",
    data: { type: "userAction", action },
  }, sequence);
}

export function sessionHistory(
  blocks: AgentMessageBlock[],
  metrics: AgentRunMetrics | null = null,
): SessionHistoryResponse {
  const messages: AgentMessage[] = [];
  let agentBlocks: AgentMessageBlock[] = [];
  let currentUserMessage: AgentMessage | undefined;
  const flushAgentBlocks = () => {
    if (agentBlocks.length > 0) {
      messages.push(message(`msg-agent-${messages.length}`, "turn-1", "agent", agentBlocks));
      agentBlocks = [];
    }
  };
  for (const entry of blocks) {
    if (entry.content.type !== "userContent") {
      currentUserMessage = undefined;
      agentBlocks.push(entry);
      continue;
    }
    flushAgentBlocks();
    if (entry.content.data.type === "text" || !currentUserMessage) {
      currentUserMessage = message(
        `msg-${entry.blockId}`,
        "turn-1",
        "user",
        [entry],
      );
      messages.push(currentUserMessage);
    } else {
      currentUserMessage.blocks.push(entry);
    }
  }
  flushAgentBlocks();
  return {
    data: [{
      identity: { sessionId: "sess-1", turnId: "turn-1" },
      completedEventSeq: 9,
      completedAt: 2,
      result: {
        status: "completed",
        result: {
          messages,
          metrics: metrics ?? executionMetrics(),
          completion: { stopReason: "endTurn" },
        },
      },
    }],
    nextCursor: null,
  };
}

export function historyTextItem(
  itemId: string,
  kind: "assistant_text" | "user_input",
  phase: "commentary" | "final_answer" | "pending" | "input",
  text: string,
): AgentMessageBlock {
  const content: AgentMessageBlockContent = kind === "user_input"
    ? { type: "userContent", data: { type: "text", text } }
    : {
        type: "assistantText",
        data: { phase: assistantProtocolPhase(phase), text },
      };
  return block(itemId, content, "completed", 1, 2);
}

export function historyUserInputItem(
  itemId: string,
  text: string,
  fileIds: string[],
): AgentMessageBlock[] {
  return [
    historyTextItem(itemId, "user_input", "input", text),
    ...fileIds.map((fileId, index) =>
      block(
        `${itemId}-file-${index}`,
        { type: "userContent", data: { type: "file", fileId } },
        "completed",
        1,
        2,
      )),
  ];
}

export function historyToolItems(itemId: string, output: string): AgentMessageBlock[] {
  return [
    block(
      itemId,
      {
        type: "toolCall",
        data: {
          callId: `call-${itemId}`,
          name: "Shell",
          arguments: { state: "complete", value: {} },
          progress: [],
        },
      },
      "completed",
      1,
      2,
    ),
    block(
      `${itemId}-result`,
      {
        type: "toolResult",
        data: {
          callId: `call-${itemId}`,
          toolName: "Shell",
          content: [{ type: "text", text: output }],
          isError: false,
        },
      },
      "completed",
      1,
      2,
    ),
  ];
}

export function executionMetrics(
  totalTokens = 0,
  overrides: Partial<Pick<AgentRunMetrics, "modelRounds" | "toolCalls" | "toolSuccesses" | "toolErrors">> = {},
): AgentRunMetrics {
  return {
    modelRounds: overrides.modelRounds ?? 1,
    toolCalls: overrides.toolCalls ?? 0,
    toolSuccesses: overrides.toolSuccesses ?? 0,
    toolErrors: overrides.toolErrors ?? 0,
    usage: { ...usage(), total_tokens: totalTokens },
    context: null,
    durationMs: null,
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

export function activeTurn(overrides: Partial<TimelineActiveTurn>): TimelineActiveTurn {
  return {
    identity: { sessionId: "sess-1", turnId: "turn-1" },
    initialMessageId: "msg-1",
    state: "running",
    lastEventId: "ev_0",
    updatedAt: 0,
    ...overrides,
  };
}

export function pendingUserAction(overrides: Partial<PendingUserAction> = {}): PendingUserAction {
  return {
    actionId: "action-1",
    callId: "call-tool-1",
    itemId: "tool-1",
    state: "pending",
    params: { type: "dynamicToolResult", params: { toolName: "Shell" } },
    ...overrides,
  };
}

function block(
  blockId: string,
  content: AgentMessageBlockContent,
  status: AgentMessageBlock["status"],
  startedAt: number,
  completedAt: number | null,
): AgentMessageBlock {
  return { blockId, content, status, startedAt, completedAt };
}

function message(
  messageId: string,
  turnId: string,
  role: AgentMessage["role"],
  blocks: AgentMessageBlock[],
  status: AgentMessage["status"] = "completed",
): AgentMessage {
  return { messageId, turnId, role, status, blocks, createdAt: 1, completedAt: 2 };
}

function assistantProtocolPhase(
  phase: "commentary" | "final_answer" | "pending" | "acting" | "input" | "thinking",
): AgentAssistantTextPhase {
  if (phase === "final_answer") {
    return "finalAnswer";
  }
  if (phase === "pending") {
    return "pending";
  }
  return "commentary";
}

function compaction(
  stage: ContextCompactionStage,
  messageText: string | null,
): AgentContextCompaction {
  return {
    trigger: "threshold_exceeded",
    stage,
    outcome: null,
    metrics: null,
    message: messageText,
  };
}
