import type {
  ChatRenderBlock,
  ChatTimelineItem,
  StreamSessionState,
} from "@/shared/types/ui";
import type { TalosTimelinePhase } from "@talos/sdk-core";

function ensureTimelineBlock(blocks: StreamSessionState["blocks"], sessionId: string) {
  const existingIndex = blocks.findIndex((block) => block.kind === "timeline");
  if (existingIndex >= 0) {
    return existingIndex;
  }
  blocks.push({
    id: `timeline-${sessionId}`,
    kind: "timeline",
    items: [],
    live: true,
  });
  return blocks.length - 1;
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

export function upsertTimelineItemInStream(
  stream: StreamSessionState,
  sessionId: string,
  item: ChatTimelineItem,
) {
  const blocks = [...stream.blocks];
  const timelineIndex = ensureTimelineBlock(blocks, sessionId);
  const timeline = blocks[timelineIndex];
  if (timeline.kind !== "timeline") {
    return stream;
  }
  const existingIndex = timeline.items.findIndex((entry) => entry.itemId === item.itemId);
  const items = [...timeline.items];
  if (existingIndex >= 0) {
    items[existingIndex] = mergeTimelineItem(items[existingIndex], item);
  } else {
    items.push(item);
  }
  blocks[timelineIndex] = { ...timeline, items, live: true };
  return { ...stream, isStreaming: true, blocks };
}

export function appendTimelineItemTextInStream(
  stream: StreamSessionState,
  sessionId: string,
  sourceItem: ChatTimelineItem,
  chunk: string,
  sequence?: number,
) {
  const blocks = [...stream.blocks];
  const timelineIndex = ensureTimelineBlock(blocks, sessionId);
  const timeline = blocks[timelineIndex];
  if (timeline.kind !== "timeline") {
    return stream;
  }
  const sourceItemId = sourceItem.sourceItemId ?? sourceItem.itemId;
  const sameSource = (item: ChatTimelineItem): item is TextTimelineItem =>
    isTextTimelineItem(item) && (item.sourceItemId ?? item.itemId) === sourceItemId;
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
  blocks[timelineIndex] = { ...timeline, items, live: true };
  return { ...stream, isStreaming: true, blocks };
}

export function commitTimelineAssistantTextInStream(
  stream: StreamSessionState,
  itemId: string,
  phase: TalosTimelinePhase,
) {
  let movedText = "";
  const blocks: ChatRenderBlock[] = [];
  for (const block of stream.blocks) {
    if (block.kind !== "timeline") {
      blocks.push(block);
      continue;
    }
    const items: ChatTimelineItem[] = [];
    for (const item of block.items) {
      const sameSource = item.itemId === itemId || item.sourceItemId === itemId;
      if (!sameSource) {
        items.push(item);
        continue;
      }
      if (item.kind !== "tool_call") {
        movedText += item.text;
      }
      if (phase !== "final_answer") {
        items.push({ ...item, phase });
      }
    }
    if (items.length === 0 && phase === "final_answer") {
      continue;
    }
    blocks.push({ ...block, items, live: phase !== "final_answer" });
  }
  const nextBlocks = [...blocks];
  if (phase === "final_answer" && movedText.trim()) {
    const lastBlock = nextBlocks.at(-1);
    if (lastBlock?.kind === "assistant") {
      nextBlocks[nextBlocks.length - 1] = {
        ...lastBlock,
        text: lastBlock.text + movedText,
        live: true,
      };
    } else {
      nextBlocks.push({
        id: `assistant-${Date.now()}-${nextBlocks.length}`,
        kind: "assistant",
        text: movedText,
        live: true,
      });
    }
  }
  return { ...stream, isStreaming: true, blocks: nextBlocks };
}

export function updateTimelineToolInStream(
  stream: StreamSessionState,
  itemId: string,
  payload: Partial<Extract<ChatTimelineItem, { kind: "tool_call" }>>,
) {
  const blocks = stream.blocks.map((block) => {
    if (block.kind !== "timeline") {
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
  itemId: string,
  status: ChatTimelineItem["status"],
  completedAtMs: number,
) {
  const blocks = stream.blocks.map((block) => {
    if (block.kind !== "timeline") {
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
