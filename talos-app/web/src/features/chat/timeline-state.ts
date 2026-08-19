import type {
  ChatTurnMetrics,
  ChatTimelineItem,
  StreamSessionState,
} from "@/shared/types/ui";
import type { TalosTimelinePhase } from "@talos/sdk-core";
import type { ContextCompactionStage } from "@/shared/types/protocol";

const ACTIVE_CONTEXT_COMPACTION_STAGES: ReadonlySet<ContextCompactionStage> = new Set([
  "preparing",
  "summarizing",
  "rebuilding_prompt",
  "trimming",
]);

export function activeContextCompactionStage(
  stream: StreamSessionState | undefined,
): ContextCompactionStage | undefined {
  if (!stream) {
    return undefined;
  }
  for (const block of stream?.blocks ?? []) {
    if (block.kind !== "timeline" || !block.live) {
      continue;
    }
    for (let index = block.items.length - 1; index >= 0; index -= 1) {
      const item = block.items[index];
      if (
        item.kind === "context_compaction" &&
        item.status === "running" &&
        ACTIVE_CONTEXT_COMPACTION_STAGES.has(item.stage)
      ) {
        return item.stage;
      }
    }
  }
  return undefined;
}

export function contextCompactionStageLabel(stage: ContextCompactionStage) {
  switch (stage) {
    case "preparing":
      return "preparing";
    case "summarizing":
      return "summarizing";
    case "rebuilding_prompt":
      return "rebuilding prompt";
    case "trimming":
      return "trimming";
    case "completed":
      return "completed";
    case "failed":
      return "failed";
  }
}

function timelineBlockPrefix(sessionId: string, messageId: string) {
  return `timeline-${sessionId}-${messageId}-`;
}

function timelineBlockId(
  blocks: StreamSessionState["blocks"],
  sessionId: string,
  messageId: string,
) {
  const userPrefix = `user-${messageId}-`;
  let segmentId: string | undefined;
  for (let index = blocks.length - 1; index >= 0; index -= 1) {
    const block = blocks[index];
    if (block.kind === "user" && block.id.startsWith(userPrefix)) {
      segmentId = block.id.slice(userPrefix.length);
      break;
    }
  }
  return segmentId
    ? `${timelineBlockPrefix(sessionId, messageId)}${segmentId}`
    : `timeline-${sessionId}-${messageId}`;
}

function isMessageTimelineBlock(
  block: StreamSessionState["blocks"][number],
  sessionId: string,
  messageId: string,
): block is Extract<StreamSessionState["blocks"][number], { kind: "timeline" }> {
  const baseId = `timeline-${sessionId}-${messageId}`;
  return (
    block.kind === "timeline" &&
    (block.id === baseId || block.id.startsWith(timelineBlockPrefix(sessionId, messageId)))
  );
}

function ensureTimelineBlock(
  stream: StreamSessionState,
  blocks: StreamSessionState["blocks"],
  sessionId: string,
  messageId: string,
) {
  const id = timelineBlockId(blocks, sessionId, messageId);
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
    metrics.model_rounds > 0 ||
    metrics.tool_calls > 0 ||
    metrics.tool_successes > 0 ||
    metrics.tool_errors > 0 ||
    metrics.usage.total_tokens > 0 ||
    metrics.duration_ms != null
  );
}

function mergeTimelineItem(current: ChatTimelineItem, next: ChatTimelineItem): ChatTimelineItem {
  if (current.kind === "tool_call" && next.kind === "tool_call") {
    const status =
      current.status === "waiting_user_action" && next.status === "running"
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
  messageId: string,
  item: ChatTimelineItem,
) {
  const blocks = [...stream.blocks];
  const sourceItemId = timelineSourceId(item);
  const existingTimelineIndex = blocks.findIndex(
    (block) =>
      isMessageTimelineBlock(block, sessionId, messageId) &&
      block.items.some((entry) => isSameTimelineSource(entry, sourceItemId)),
  );
  const timelineIndex =
    existingTimelineIndex >= 0
      ? existingTimelineIndex
      : ensureTimelineBlock(stream, blocks, sessionId, messageId);
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

export function appendTimelineItemTextInStream(
  stream: StreamSessionState,
  sessionId: string,
  messageId: string,
  sourceItem: ChatTimelineItem,
  chunk: string,
  sequence?: number,
) {
  const blocks = [...stream.blocks];
  const sourceItemId = timelineSourceId(sourceItem);
  const existingTimelineIndex = blocks.findIndex(
    (block) =>
      isMessageTimelineBlock(block, sessionId, messageId) &&
      block.items.some((entry) => isSameTimelineSource(entry, sourceItemId)),
  );
  const timelineIndex =
    existingTimelineIndex >= 0
      ? existingTimelineIndex
      : ensureTimelineBlock(stream, blocks, sessionId, messageId);
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
  messageId: string,
  itemId: string,
  phase: TalosTimelinePhase,
) {
  const blocks = stream.blocks.map((block) => {
    if (!isMessageTimelineBlock(block, sessionId, messageId)) {
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
            phase === "final_answer" && item.phase === "unknown" ? true : item.replayFinalAnswer,
        };
      }),
    };
  });
  return { ...stream, isStreaming: true, blocks };
}

export function updateTimelineToolInStream(
  stream: StreamSessionState,
  sessionId: string,
  messageId: string,
  itemId: string,
  payload: Partial<Extract<ChatTimelineItem, { kind: "tool_call" }>>,
) {
  const blocks = stream.blocks.map((block) => {
    if (!isMessageTimelineBlock(block, sessionId, messageId)) {
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
  messageId: string,
  itemId: string,
  status: ChatTimelineItem["status"],
  completedAtMs: number,
) {
  const blocks = stream.blocks.map((block) => {
    if (!isMessageTimelineBlock(block, sessionId, messageId)) {
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
  messageId: string,
  metrics: ChatTurnMetrics,
) {
  const activeTimelineBlockId = timelineBlockId(stream.blocks, sessionId, messageId);
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
