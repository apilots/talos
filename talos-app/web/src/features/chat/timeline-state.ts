import type {
  ChatTurnMetrics,
  ChatTimelineItem,
  StreamSessionState,
} from "@/shared/types/ui";
import type { TalosTimelinePhase } from "@talos/sdk-core";

function timelineBlockId(sessionId: string, messageId: string) {
  return `timeline-${sessionId}-${messageId}`;
}

function ensureTimelineBlock(
  stream: StreamSessionState,
  blocks: StreamSessionState["blocks"],
  sessionId: string,
  messageId: string,
) {
  const id = timelineBlockId(sessionId, messageId);
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
      current.status === "waiting_dynamic_input" && next.status === "running"
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
  const timelineIndex = ensureTimelineBlock(stream, blocks, sessionId, messageId);
  const timeline = blocks[timelineIndex];
  if (timeline.kind !== "timeline") {
    return stream;
  }
  const sourceItemId = timelineSourceId(item);
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
  const timelineIndex = ensureTimelineBlock(stream, blocks, sessionId, messageId);
  const timeline = blocks[timelineIndex];
  if (timeline.kind !== "timeline") {
    return stream;
  }
  const sourceItemId = timelineSourceId(sourceItem);
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
    if (block.kind !== "timeline" || block.id !== timelineBlockId(sessionId, messageId)) {
      return block;
    }
    return {
      ...block,
      items: block.items.map((item) => {
        const sameSource = item.itemId === itemId || item.sourceItemId === itemId;
        return sameSource && item.kind !== "tool_call" ? { ...item, phase } : item;
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
    if (block.kind !== "timeline" || block.id !== timelineBlockId(sessionId, messageId)) {
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
    if (block.kind !== "timeline" || block.id !== timelineBlockId(sessionId, messageId)) {
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
  let foundTimeline = false;
  const blocks = stream.blocks.map((block) => {
    if (block.kind !== "timeline" || block.id !== timelineBlockId(sessionId, messageId)) {
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
      id: timelineBlockId(sessionId, messageId),
      kind: "timeline",
      items: [],
      metrics,
      live: metrics.state === "running",
    });
  }
  return { ...stream, turnMetrics: metrics, blocks };
}
