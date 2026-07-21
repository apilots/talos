import type { TalosTimelineContent } from "@talos/sdk-browser";

export type ContextCompactionStage = Extract<
  TalosTimelineContent,
  { type: "context_compaction" }
>["stage"];

export type {
  TalosContentItem,
  TalosTimelineItemKind,
  TalosTimelineItemStatus,
  TalosTimelinePhase,
  TalosTimelineContent,
  TalosPendingUserAction,
  RunItem,
  SessionInfo,
  TalosToolProgressKind,
  TalosTurnExecutionMetrics,
} from "@talos/sdk-browser";
