import type { TalosTimelineContent } from "@talos/sdk-browser";

export type ContextCompactionContent = Extract<
  TalosTimelineContent,
  { type: "context_compaction" }
>;
export type ContextCompactionTrigger = ContextCompactionContent["trigger"];
export type ContextCompactionStage = ContextCompactionContent["stage"];
export type ContextCompactionOutcome = ContextCompactionContent["outcome"];
export type ContextCompactionMetrics = ContextCompactionContent["metrics"];

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
