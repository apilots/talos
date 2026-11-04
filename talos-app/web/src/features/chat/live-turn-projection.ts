import type {
  ActiveTurnSnapshot,
  AgentEvent,
  AgentMessage,
  AgentMessageBlock,
  AgentPendingInput,
  AgentRunMetrics,
  AgentTurnRef,
} from "@talos/sdk-core";

/**
 * Web projection of one live Agent Turn event stream.
 *
 * The server remains authoritative. This accumulator only applies ordered
 * events, replaces provisional state with MessageCompleted snapshots,
 * and exposes immutable copies for the Chat UI.
 */
export class LiveTurnProjection {
  private identityValue: AgentTurnRef | undefined;
  private lastSequenceValue = 0;
  private readonly messagesById = new Map<string, AgentMessage>();
  private readonly blocksByMessageId = new Map<string, Map<string, AgentMessageBlock>>();
  private metricsValue: AgentRunMetrics | undefined;
  private pendingInputValue: AgentPendingInput | undefined;

  restore(snapshot: ActiveTurnSnapshot) {
    this.reset(snapshot.identity);
    for (const message of snapshot.messages) this.upsertMessage(message);
    this.lastSequenceValue = snapshot.lastSequence;
    this.metricsValue = clone(snapshot.metrics);
    this.pendingInputValue = snapshot.pendingInput ? clone(snapshot.pendingInput) : undefined;
  }

  apply(event: AgentEvent): boolean {
    if (!sameIdentity(this.identityValue, event.identity)) {
      this.reset(event.identity);
    }
    if (event.sequence <= this.lastSequenceValue) {
      return false;
    }

    switch (event.payload.type) {
      case "turnStarted":
        break;
      case "messageStarted":
        this.upsertMessage(event.payload.data.message);
        break;
      case "messageBlockStarted":
      case "messageBlockCompleted":
        this.upsertBlock(event.payload.data.messageId, event.payload.data.block);
        break;
      case "messageBlockDelta":
        this.applyBlockDelta(
          event.payload.data.messageId,
          event.payload.data.blockId,
          event.payload.data.delta,
        );
        break;
      case "messageCompleted":
        this.upsertMessage(event.payload.data);
        break;
      case "waitingForInput":
        this.pendingInputValue = clone(event.payload.data);
        break;
      case "metricsUpdated":
        this.metricsValue = clone(event.payload.data);
        break;
      case "turnCompleted":
      case "turnFailed":
        this.pendingInputValue = undefined;
        break;
    }
    this.lastSequenceValue = event.sequence;
    return true;
  }

  reset(identity?: AgentTurnRef) {
    this.identityValue = identity ? clone(identity) : undefined;
    this.lastSequenceValue = 0;
    this.messagesById.clear();
    this.blocksByMessageId.clear();
    this.metricsValue = undefined;
    this.pendingInputValue = undefined;
  }

  message(messageId: string): AgentMessage | undefined {
    const message = this.messagesById.get(messageId);
    if (!message) {
      return undefined;
    }
    return clone({
      ...message,
      blocks: [...(this.blocksByMessageId.get(messageId)?.values() ?? [])],
    });
  }

  block(messageId: string, blockId: string): AgentMessageBlock | undefined {
    const block = this.blocksByMessageId.get(messageId)?.get(blockId);
    return block ? clone(block) : undefined;
  }

  metrics(): AgentRunMetrics | undefined {
    return this.metricsValue ? clone(this.metricsValue) : undefined;
  }

  pendingInput(): AgentPendingInput | undefined {
    return this.pendingInputValue ? clone(this.pendingInputValue) : undefined;
  }

  toolCall(callId: string): { messageId: string; block: AgentMessageBlock } | undefined {
    for (const [messageId, blocks] of this.blocksByMessageId) {
      const block = [...blocks.values()].find(
        (candidate) =>
          candidate.content.type === "toolCall" && candidate.content.data.callId === callId,
      );
      if (block) {
        return { messageId, block: clone(block) };
      }
    }
    return undefined;
  }

  private upsertMessage(message: AgentMessage) {
    const copy = clone(message);
    this.messagesById.set(message.messageId, { ...copy, blocks: [] });
    this.blocksByMessageId.set(message.messageId,
      new Map(copy.blocks.map((block) => [block.blockId, block])));
  }

  private upsertBlock(messageId: string, block: AgentMessageBlock) {
    let blocks = this.blocksByMessageId.get(messageId);
    if (!blocks) { blocks = new Map(); this.blocksByMessageId.set(messageId, blocks); }
    blocks.set(block.blockId, clone(block));
  }

  private applyBlockDelta(
    messageId: string,
    blockId: string,
    delta: Extract<AgentEvent["payload"], { type: "messageBlockDelta" }>["data"]["delta"],
  ) {
    const block = this.blocksByMessageId.get(messageId)?.get(blockId);
    if (!block) {
      return;
    }
    switch (delta.type) {
      case "text":
        if (block.content.type === "assistantText" || block.content.type === "reasoning") {
          block.content.data.text += delta.data;
        }
        return;
      case "assistantTextPhase":
        if (block.content.type === "assistantText") {
          block.content.data.phase = delta.data;
        }
        return;
      case "toolCallArguments":
        if (
          block.content.type === "toolCall" &&
          block.content.data.arguments.state === "streaming"
        ) {
          block.content.data.arguments.jsonFragment += delta.data.jsonFragment;
        }
        return;
      case "toolProgress":
        if (block.content.type === "toolCall") {
          block.content.data.progress.push(clone(delta.data));
        }
        return;
      case "contextCompaction":
        if (block.content.type === "contextCompaction") {
          block.content.data.stage = delta.data.stage ?? block.content.data.stage;
          block.content.data.outcome = delta.data.outcome ?? block.content.data.outcome;
          block.content.data.metrics = delta.data.metrics ?? block.content.data.metrics;
          block.content.data.message = delta.data.message ?? block.content.data.message;
        }
        return;
    }
  }
}

function sameIdentity(left: AgentTurnRef | undefined, right: AgentTurnRef) {
  return left?.sessionId === right.sessionId && left.turnId === right.turnId;
}

function clone<T>(value: T): T {
  return structuredClone(value);
}
