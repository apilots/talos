import type { ChatRenderBlock, ChatTimelineItem, ChatToolStatus } from "@/shared/types/ui";
import { toolActivity } from "./toolDisplay";

type ToolItem = Extract<ChatTimelineItem, { kind: "tool_call" }>;
type ProcessOutcome = Pick<Extract<ChatRenderBlock, { kind: "timeline" }>, "error" | "metrics">;

export interface TimelineActivity {
  kind: "activity";
  itemId: string;
  items: ChatTimelineItem[];
}

export const TIMELINE_PAGE_SIZE = 64;

export function groupTimelineItems(items: ChatTimelineItem[]) {
  const rows: Array<TimelineActivity | ChatTimelineItem> = [];
  let lastGroupedTool: ToolItem | undefined;

  for (const item of items) {
    const isRoutineTool = item.kind === "tool_call" && item.status === "completed" && !item.isError;
    const isThinking = item.kind === "reasoning";
    if (!isRoutineTool && !isThinking) {
      rows.push(item);
      continue;
    }

    const previous = rows.at(-1);
    if (previous?.kind === "activity") {
      const toolGroup = previous.items[0]?.kind === "tool_call";
      const belongsToGroup = isRoutineTool ? toolGroup : !toolGroup || thinkingDuringCommand(item, lastGroupedTool);
      if (belongsToGroup) {
        previous.items.push(item);
        if (isRoutineTool) lastGroupedTool = item;
        continue;
      }
    }
    rows.push({ kind: "activity", itemId: item.itemId, items: [item] });
    if (isRoutineTool) lastGroupedTool = item;
  }
  return rows;
}

function thinkingDuringCommand(item: ChatTimelineItem, tool: ToolItem | undefined) {
  if (!tool || toolActivity(tool.toolName) !== "commands" || item.status !== "completed") return false;
  const execution = tool.executionMetrics;
  if (execution?.startedAtMs == null || execution.finishedAtMs == null || execution.startedAtMs <= 0) return false;
  if (item.startedAtMs <= 0 || item.completedAtMs == null || item.completedAtMs < item.startedAtMs) return false;

  // Reasoning block timestamps are rounded down to seconds. Require the entire completion second.
  return item.startedAtMs >= execution.startedAtMs && item.completedAtMs + 1000 <= execution.finishedAtMs;
}

export function timelineItemDuration(item: ChatTimelineItem) {
  return item.completedAtMs == null || item.startedAtMs <= 0
    ? null
    : Math.max(0, item.completedAtMs - item.startedAtMs);
}

export function timelineProcessTiming(items: ChatTimelineItem[], { error, metrics }: ProcessOutcome) {
  let cancelled = false;
  let earliest = Infinity;
  let latest = 0;
  for (const item of items) {
    cancelled ||= item.status === "cancelled";
    if (item.startedAtMs > 0) earliest = Math.min(earliest, item.startedAtMs);
    latest = Math.max(latest, item.completedAtMs ?? 0);
  }
  const outcome = error || metrics?.state === "failed"
    ? "processFailed"
    : cancelled && metrics?.state !== "completed" ? "processStopped" : "processWorked";
  return {
    outcome,
    startedAtMs: Number.isFinite(earliest) ? earliest : null,
    durationMs: metrics?.durationMs ?? (latest >= earliest ? latest - earliest : null),
  } as const;
}

export function timelineToolStatus(item: ToolItem): ChatToolStatus {
  switch (item.status) {
    case "queued": return "queued";
    case "running": return "running";
    case "waitingForInput": return "waiting_dynamic_input";
    case "completed": return item.isError ? "error" : "done";
    case "failed": return "error";
    case "cancelled": return "cancelled";
  }
}
