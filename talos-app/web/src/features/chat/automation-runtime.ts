import type { ActiveTurnSnapshot } from "@talos/sdk-browser";
import type { AutomationTurnTiming } from "@/features/automation/model";
import type { ChatRenderBlock, StreamSessionState } from "@/shared/types/ui";

/** Only an observed live Turn and its canonical triggering message start a clock. */
export function automationTurnTiming(
  sessionId: string,
  activeTurnId: string | undefined,
  stream: StreamSessionState | undefined,
  snapshot: ActiveTurnSnapshot | null | undefined,
): AutomationTurnTiming | undefined {
  if (!activeTurnId) return;
  const timelineId = `timeline-${sessionId}-${activeTurnId}`;
  if (stream?.blocks.some((block) => block.kind === "timeline"
    && (block.id === timelineId || block.id.startsWith(`${timelineId}-`))
    && (block.metrics?.state === "completed" || block.metrics?.state === "failed"))) return;
  const current = stream?.activeTurnId === activeTurnId ? stream : undefined;
  const loaded = snapshot?.identity.sessionId === sessionId && snapshot.identity.turnId === activeTurnId ? snapshot : undefined;
  const input = current?.automationInput ?? loaded?.automationInput;
  if (!input) return;
  const block = current?.blocks.find((entry) => entry.kind === "user" && entry.id === `user-${activeTurnId}-${input.messageId}`);
  const startedAt = block?.kind === "user" ? block.createdAt : undefined;
  const createdAt = startedAt ?? loaded?.messages.find((message) => message.role === "user" && message.turnId === activeTurnId && message.messageId === input.messageId)?.createdAt;
  if (createdAt === undefined || !Number.isFinite(createdAt)) return;
  return { turnId: activeTurnId, origin: input.origin, startedAt: createdAt };
}

export function bindUserInputMetadata(stream: StreamSessionState, blocks: ChatRenderBlock[]) {
  const input = stream.automationInput;
  const pending = stream.pendingInput;
  if (!stream.activeTurnId || (!input && !pending?.activationMode)) return blocks;
  const originId = input ? `user-${stream.activeTurnId}-${input.messageId}` : undefined;
  const activationId = pending?.serverMessageId ? `user-${stream.activeTurnId}-${pending.serverMessageId}` : undefined;
  return blocks.map((block) => {
    if (block.kind !== "user") return block;
    const origin = block.id === originId ? input?.origin : undefined;
    const activationMode = !block.activationMode && block.id === activationId ? pending?.activationMode : undefined;
    return origin || activationMode ? { ...block, ...(origin ? { automationOrigin: origin } : {}), ...(activationMode ? { activationMode } : {}) } : block;
  });
}
