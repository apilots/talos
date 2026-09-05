import type {
  ChatTimelinePhase,
  ChatTurnMetrics,
  ChatTimelineItem,
  StreamSessionState,
} from "@/shared/types/ui";
import type { ContextCompactionStage } from "@/shared/types/protocol";

export type ChatContextCompactionItem = Extract<
  ChatTimelineItem,
  { kind: "context_compaction" }
>;

const ACTIVE_CONTEXT_COMPACTION_STAGES: ReadonlySet<ContextCompactionStage> = new Set([
  "preparing",
  "summarizing",
  "rebuilding_prompt",
  "trimming",
]);

export function isContextCompactionRunning(item: ChatContextCompactionItem) {
  return item.status === "running" && ACTIVE_CONTEXT_COMPACTION_STAGES.has(item.stage);
}

export function latestContextCompaction(
  stream: StreamSessionState | undefined,
): ChatContextCompactionItem | undefined {
  if (!stream) {
    return undefined;
  }
  let latest: ChatContextCompactionItem | undefined;
  for (const block of stream.blocks) {
    if (block.kind !== "timeline") {
      continue;
    }
    for (const item of block.items) {
      if (
        item.kind === "context_compaction" &&
        (!latest || item.startedAtMs >= latest.startedAtMs)
      ) {
        latest = item;
      }
    }
  }
  return latest;
}

export function activeContextCompaction(
  stream: StreamSessionState | undefined,
): ChatContextCompactionItem | undefined {
  if (!stream) {
    return undefined;
  }
  for (let blockIndex = stream.blocks.length - 1; blockIndex >= 0; blockIndex -= 1) {
    const block = stream.blocks[blockIndex];
    if (block.kind !== "timeline" || !block.live) {
      continue;
    }
    for (let itemIndex = block.items.length - 1; itemIndex >= 0; itemIndex -= 1) {
      const item = block.items[itemIndex];
      if (
        item.kind === "context_compaction" &&
        isContextCompactionRunning(item)
      ) {
        return item;
      }
    }
  }
  return undefined;
}

function timelineBlockPrefix(sessionId: string, turnId: string) {
  return `timeline-${sessionId}-${turnId}-`;
}

function timelineBlockId(
  blocks: StreamSessionState["blocks"],
  sessionId: string,
  turnId: string,
) {
  const userPrefix = `user-${turnId}-`;
  let segmentId: string | undefined;
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (block.kind === "user" && block.id.startsWith(userPrefix)) {
      segmentId = block.id.slice(userPrefix.length);
      break;
    }
  }
  return segmentId
    ? `${timelineBlockPrefix(sessionId, turnId)}${segmentId}`
    : `timeline-${sessionId}-${turnId}`;
}

function isTurnTimelineBlock(
  block: StreamSessionState["blocks"][number],
  sessionId: string,
  turnId: string,
): block is Extract<StreamSessionState["blocks"][number], { kind: "timeline" }> {
  const baseId = `timeline-${sessionId}-${turnId}`;
  return (
    block.kind === "timeline" &&
    (block.id === baseId || block.id.startsWith(timelineBlockPrefix(sessionId, turnId)))
  );
}

function ensureTimelineBlock(
  stream: StreamSessionState,
  blocks: StreamSessionState["blocks"],
  sessionId: string,
  turnId: string,
) {
  const id = timelineBlockId(blocks, sessionId, turnId);
  const existingIndex = blocks.findIndex((block) => block.kind === "timeline" && block.id === id);
  if (existingIndex >= 0) {
    return existingIndex;
  }
  blocks.push({
    id,
    kind: "timeline",
    items: [],
    metrics: stream.turnMetrics,
    live: true,
  });
  return blocks.length - 1;
}

function shouldCreateMetricsBlock(metrics: ChatTurnMetrics) {
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

function mergeTimelineItem(current: ChatTimelineItem, next: ChatTimelineItem): ChatTimelineItem {
  if (current.kind === "tool_call" && next.kind === "tool_call") {
    const status =
      current.status === "waitingForInput" && next.status === "running"
        ? current.status
        : next.status;
    return {
      ...current,
      ...next,
      status,
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

type TextTimelineItem = Extract<ChatTimelineItem, { text: string }>;

function isTextTimelineItem(item: ChatTimelineItem): item is TextTimelineItem {
  return item.kind !== "tool_call";
}

function timelineSourceId(item: ChatTimelineItem) {
  return item.sourceItemId ?? item.itemId;
}

function isSameTimelineSource(item: ChatTimelineItem, sourceItemId: string) {
  return timelineSourceId(item) === sourceItemId;
}

export function upsertTimelineItemInStream(
  stream: StreamSessionState,
  sessionId: string,
  turnId: string,
  item: ChatTimelineItem,
) {
  const blocks = [...stream.blocks];
  const sourceItemId = timelineSourceId(item);
  const existingTimelineIndex = blocks.findIndex(
    (block) =>
      isTurnTimelineBlock(block, sessionId, turnId) &&
      block.items.some((entry) => isSameTimelineSource(entry, sourceItemId)),
  );
  const timelineIndex =
    existingTimelineIndex >= 0
      ? existingTimelineIndex
      : ensureTimelineBlock(stream, blocks, sessionId, turnId);
  const timeline = blocks[timelineIndex];
  if (timeline.kind !== "timeline") {
    return stream;
  }
  const existingIndex = timeline.items.findIndex((entry) => isSameTimelineSource(entry, sourceItemId));
  const items = [...timeline.items];
  if (existingIndex >= 0) {
    items[existingIndex] = mergeTimelineItem(items[existingIndex], item);
  } else {
    items.push(item);
  }
  blocks[timelineIndex] = { ...timeline, items, metrics: stream.turnMetrics ?? timeline.metrics, live: true };
  return { ...stream, isStreaming: true, blocks };
}

export function replaceTimelineItemInStream(
  stream: StreamSessionState,
  sessionId: string,
  turnId: string,
  item: ChatTimelineItem,
) {
  const sourceItemId = timelineSourceId(item);
  const blocks = [...stream.blocks];
  let replaced = false;
  for (let blockIndex = 0; blockIndex < blocks.length; blockIndex += 1) {
    const block = blocks[blockIndex];
    if (!isTurnTimelineBlock(block, sessionId, turnId)) {
      continue;
    }
    const firstIndex = block.items.findIndex((entry) =>
      isSameTimelineSource(entry, sourceItemId),
    );
    if (firstIndex < 0) {
      continue;
    }
    const items = block.items.filter((entry) => !isSameTimelineSource(entry, sourceItemId));
    if (!replaced) {
      items.splice(firstIndex, 0, item);
      replaced = true;
    }
    blocks[blockIndex] = { ...block, items };
  }
  return replaced
    ? { ...stream, blocks }
    : upsertTimelineItemInStream(stream, sessionId, turnId, item);
}

export function appendTimelineItemTextInStream(
  stream: StreamSessionState,
  sessionId: string,
  turnId: string,
  sourceItem: ChatTimelineItem,
  chunk: string,
  sequence?: number,
) {
  const blocks = [...stream.blocks];
  const sourceItemId = timelineSourceId(sourceItem);
  const existingTimelineIndex = blocks.findIndex(
    (block) =>
      isTurnTimelineBlock(block, sessionId, turnId) &&
      block.items.some((entry) => isSameTimelineSource(entry, sourceItemId)),
  );
  const timelineIndex =
    existingTimelineIndex >= 0
      ? existingTimelineIndex
      : ensureTimelineBlock(stream, blocks, sessionId, turnId);
  const timeline = blocks[timelineIndex];
  if (timeline.kind !== "timeline") {
    return stream;
  }
  const sameSource = (item: ChatTimelineItem): item is TextTimelineItem =>
    isTextTimelineItem(item) && isSameTimelineSource(item, sourceItemId);
  const items = timeline.items.filter((item) => !sameSource(item) || item.text.trim() !== "");
  const lastItem = items.at(-1);
  if (lastItem && sameSource(lastItem)) {
    items[items.length - 1] = { ...lastItem, text: lastItem.text + chunk };
  } else if (sourceItem.kind !== "tool_call") {
    const hasExistingSegment = items.some(sameSource);
    items.push({
      ...sourceItem,
      itemId: hasExistingSegment ? `${sourceItemId}::segment-${sequence ?? items.length}` : sourceItemId,
      sourceItemId,
      text: chunk,
    });
  }
  blocks[timelineIndex] = { ...timeline, items, metrics: stream.turnMetrics ?? timeline.metrics, live: true };
  return { ...stream, isStreaming: true, blocks };
}

export function commitTimelineAssistantTextInStream(
  stream: StreamSessionState,
  sessionId: string,
  turnId: string,
  itemId: string,
  phase: ChatTimelinePhase,
) {
  const blocks = stream.blocks.map((block) => {
    if (!isTurnTimelineBlock(block, sessionId, turnId)) {
      return block;
    }
    return {
      ...block,
      items: block.items.map((item) => {
        const sameSource = item.itemId === itemId || item.sourceItemId === itemId;
        if (!sameSource || item.kind === "tool_call") {
          return item;
        }
        return {
          ...item,
          phase,
          replayFinalAnswer:
            phase === "final_answer" && item.phase === "pending" ? true : item.replayFinalAnswer,
        };
      }),
    };
  });
  return { ...stream, isStreaming: true, blocks };
}

export function updateTimelineToolInStream(
  stream: StreamSessionState,
  sessionId: string,
  turnId: string,
  itemId: string,
  payload: Partial<Extract<ChatTimelineItem, { kind: "tool_call" }>>,
) {
  const blocks = stream.blocks.map((block) => {
    if (!isTurnTimelineBlock(block, sessionId, turnId)) {
      return block;
    }
    return {
      ...block,
      live: true,
      items: block.items.map((item) => {
        if (item.itemId !== itemId || item.kind !== "tool_call") {
          return item;
        }
        return { ...item, ...payload, progress: payload.progress ?? item.progress };
      }),
    };
  });
  return { ...stream, isStreaming: true, blocks };
}

export function completeTimelineItemInStream(
  stream: StreamSessionState,
  sessionId: string,
  turnId: string,
  itemId: string,
  status: ChatTimelineItem["status"],
  completedAtMs: number,
) {
  const blocks = stream.blocks.map((block) => {
    if (!isTurnTimelineBlock(block, sessionId, turnId)) {
      return block;
    }
    return {
      ...block,
      items: block.items.map((item) =>
        item.itemId === itemId || item.sourceItemId === itemId
          ? { ...item, status, completedAtMs }
          : item,
      ),
    };
  });
  return { ...stream, blocks };
}

export function setTimelineLiveInStream(stream: StreamSessionState, live: boolean) {
  const blocks = stream.blocks.map((block) =>
    block.kind === "timeline" ? { ...block, live } : block,
  );
  return { ...stream, blocks };
}

export function setTurnMetricsInStream(
  stream: StreamSessionState,
  sessionId: string,
  turnId: string,
  metrics: ChatTurnMetrics,
) {
  const activeTimelineBlockId = timelineBlockId(stream.blocks, sessionId, turnId);
  let foundTimeline = false;
  const blocks = stream.blocks.map((block) => {
    if (block.kind !== "timeline" || block.id !== activeTimelineBlockId) {
      return block;
    }
    foundTimeline = true;
    return {
      ...block,
      metrics,
      live: metrics.state === "running",
    };
  });
  if (!foundTimeline && shouldCreateMetricsBlock(metrics)) {
    blocks.push({
      id: activeTimelineBlockId,
      kind: "timeline",
      items: [],
      metrics,
      live: metrics.state === "running",
    });
  }
  return { ...stream, turnMetrics: metrics, blocks };
}
