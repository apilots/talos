import type { ReactNode } from "react";
import type { PendingUserAction } from "@talos/sdk-browser";
import {
  Activity,
  Brain,
  Check,
  ChevronDown,
  CircleStop,
  LoaderCircle,
  MessageCircle,
  TriangleAlert,
  Wrench,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import type { ChatRenderBlock } from "@/shared/types/ui";
import { CopyableBlock } from "./CopyableBlock";
import { formatDuration } from "./formatDuration";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { ToolCallBlock } from "./ToolCallBlock";
import { TurnMetricsFooter } from "./TurnMetricsFooter";
import { ContextCompactionTimelineItem } from "./context-compaction/ContextCompactionTimelineItem";
import { apiErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { systemNoticeMessage } from "@/shared/i18n/systemNoticeMessage";

type TimelineBlock = Extract<ChatRenderBlock, { kind: "timeline" }>;
type TimelineItem = TimelineBlock["items"][number];
type TimelineTextItem = Exclude<TimelineItem, { kind: "tool_call" }>;

interface MessageTimelineBlockProps {
  block: TimelineBlock;
  copiedLabel: string;
  copyLabel: string;
  helpfulLabel: string;
  notHelpfulLabel: string;
  pendingUserAction?: PendingUserAction;
  renderUserAction?: (action: PendingUserAction) => ReactNode;
}

const FINAL_ANSWER_REPLAY_CHUNK_SIZE = 24;
const FINAL_ANSWER_REPLAY_INTERVAL_MS = 16;

export function MessageTimelineBlock({
  block,
  copiedLabel,
  copyLabel,
  helpfulLabel,
  notHelpfulLabel,
  pendingUserAction,
  renderUserAction,
}: MessageTimelineBlockProps) {
  const { t } = useTranslation("chat");
  const { items, id: timelineId } = block;
  const visibleItems = items.filter(
    (item) => item.kind !== "context_compaction" || item.trigger !== "user_requested",
  );
  const live = Boolean(block.live);
  const metrics = block.metrics ?? null;
  const responseItems = visibleItems.filter(
    (item): item is TimelineTextItem => item.kind !== "tool_call" && isTimelineResponseItem(item),
  );
  const processItems = visibleItems.filter((item) => !isTimelineResponseItem(item));
  const hasRunningItem = processItems.some(
    (item) => item.status === "running" || item.status === "waitingForInput",
  );
  const hasActiveTimeline = hasRunningItem || (live && processItems.length > 0 && responseItems.length === 0);
  const isAwaitingTimelineDelta = (live || hasRunningItem) && responseItems.length === 0;
  const [nowMs, setNowMs] = useState(() => Date.now());
  const totalDuration = timelineDuration(processItems, nowMs);
  const displayDuration = metrics?.durationMs ?? totalDuration;
  const requiresAttention = processItems.some((item) => item.status === "waitingForInput");
  const [open, setOpen] = useState(hasActiveTimeline);
  const wasActiveRef = useRef(hasActiveTimeline);
  const userCollapsedRef = useRef(false);

  useEffect(() => {
    if (!hasRunningItem) {
      return;
    }
    const timer = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [hasRunningItem]);

  useEffect(() => {
    if (requiresAttention) {
      userCollapsedRef.current = false;
      setOpen(true);
    } else if (!hasActiveTimeline && wasActiveRef.current) {
      userCollapsedRef.current = false;
      setOpen(false);
    } else if (hasActiveTimeline && !wasActiveRef.current && !userCollapsedRef.current) {
      setOpen(true);
    }
    wasActiveRef.current = hasActiveTimeline;
  }, [hasActiveTimeline, requiresAttention]);

  if (!block.error && processItems.length === 0 && responseItems.length === 0) {
    return null;
  }

  const processLabel = timelineProcessLabel({
    displayDuration,
    hasActiveTimeline,
    items: processItems,
    metrics,
    t,
  });

  return (
    <div className="space-y-4">
      {block.error ? (
        <div role="alert" className="flex items-start gap-2 px-1 py-2 text-sm leading-6 text-danger">
          <TriangleAlert aria-hidden="true" className="mt-1 shrink-0" size={14} />
          <p className="min-w-0 whitespace-pre-wrap break-words">{apiErrorMessage(block.error)}</p>
        </div>
      ) : null}
      {processItems.length > 0 ? (
        <details
          className="group/timeline text-ink"
          data-process-surface="rail"
          open={open}
        >
          <summary
            className="cursor-pointer list-none border-b border-border/35 px-1 py-2 marker:hidden group-open/timeline:border-b-transparent"
            onClick={(event) => {
              event.preventDefault();
              setOpen((current) => {
                const next = !current;
                userCollapsedRef.current = hasActiveTimeline && !next;
                return next;
              });
            }}
          >
            <span className="flex items-center gap-1.5 text-[13px] text-muted">
              {processLabel}
              <ChevronDown className="transition-transform duration-200 group-open/timeline:rotate-180" size={13} />
            </span>
          </summary>
          <ol className="space-y-4 border-t border-border/35 px-3 py-3.5 md:px-4">
            {processItems.map((item, index) => {
              const statusLabel = visibleTimelineItemStatus(item);
              const itemKey = timelineItemKey(timelineId, item.itemId);
              const role = timelineItemRole(item);
              return (
                <li
                  className="relative grid grid-cols-[1.55rem_minmax(0,1fr)] gap-2.5"
                  data-process-step="true"
                  data-timeline-item-key={itemKey}
                  data-timeline-role={role}
                  key={itemKey}
                >
                  {index < processItems.length - 1 || isAwaitingTimelineDelta ? (
                    <span className="absolute left-[0.72rem] top-7 h-[calc(100%-0.9rem)] w-px bg-border/65" />
                  ) : null}
                  <span className={timelineIconClass(role)}>
                    <TimelineItemIcon item={item} />
                  </span>
                  {item.kind === "tool_call" ? (
                    <div className="min-w-0">
                      <ToolCallBlock
                        argumentsText={formatArguments(item.arguments)}
                        callId={item.callId}
                        contentItems={item.contentItems}
                        output={item.output}
                        progress={item.progress}
                        prompt={item.prompt}
                        status={timelineToolStatus(item)}
                        timingMs={timelineItemDuration(item)}
                        toolName={item.toolName}
                        variant="timeline"
                      />
                      {pendingUserAction?.itemId === item.itemId
                        ? renderUserAction?.(pendingUserAction)
                        : null}
                    </div>
                  ) : item.kind === "context_compaction" ? (
                    <ContextCompactionTimelineItem item={item} />
                  ) : (
                    <article className={timelineTextCardClass(role)} data-timeline-surface="plain">
                      {role === "system" || statusLabel ? (
                        <header className="flex items-center justify-between gap-3 px-1 pb-1 pt-2">
                          {role === "system" ? (
                            <span className="truncate text-[12px] font-semibold tracking-[0.01em]">
                              {item.kind === "system_notice" ? t("systemNoticeTitle") : timelineItemTitle(item)}
                            </span>
                          ) : <span />}
                          {statusLabel ? (
                            <span className="shrink-0 text-[11px] text-muted">{statusLabel}</span>
                          ) : null}
                        </header>
                      ) : null}
                      <div className={role === "system" || statusLabel ? "px-1 pb-2.5 pt-1" : "px-1 py-2.5"}>
                        <TimelineTextContent item={item} role={role} />
                      </div>
                    </article>
                  )}
                </li>
              );
            })}
            {isAwaitingTimelineDelta ? (
              <li className="relative grid grid-cols-[1.55rem_minmax(0,1fr)] gap-2.5" key="timeline-waiting-indicator">
                <span
                  aria-label="Waiting for timeline update"
                  className="relative z-[1] mt-1 flex size-6 items-center justify-center rounded-full bg-background text-muted"
                >
                  <LoaderCircle className="animate-spin" size={13} />
                </span>
                <span className="min-h-7" />
              </li>
            ) : null}
          </ol>
        </details>
      ) : null}
      {responseItems.map((item, index) => {
        const itemKey = timelineItemKey(timelineId, item.itemId);
        const streaming = live && item.status === "running";
        return (
          <div className="min-w-0" key={itemKey}>
            <CopyableBlock
              copiedLabel={copiedLabel}
              copyLabel={copyLabel}
              copyText={item.text}
              helpfulLabel={helpfulLabel}
              notHelpfulLabel={notHelpfulLabel}
              variant="response"
            >
              <section
                className="px-1 py-2 text-ink"
                data-response-surface="plain"
                data-timeline-item-key={itemKey}
                data-timeline-response="true"
              >
                <FinalAnswerTextContent item={item} />
                {streaming ? (
                  <span
                    aria-label={t("streamingFinalAnswer")}
                    className="mt-2 inline-block size-1.5 animate-pulse rounded-full bg-brand"
                  />
                ) : null}
                {metrics && index === responseItems.length - 1 ? (
                  <TurnMetricsFooter metrics={metrics} />
                ) : null}
              </section>
            </CopyableBlock>
          </div>
        );
      })}
    </div>
  );
}

function isTimelineResponseItem(item: TimelineItem) {
  return item.kind === "assistant_text" && item.phase === "final_answer";
}

function timelineItemRole(item: TimelineItem) {
  if (item.kind === "tool_call") {
    return "tool";
  }
  if (item.kind === "reasoning") {
    return "thinking";
  }
  if (item.kind === "assistant_text" && (item.phase === "commentary" || item.phase === "pending")) {
    return "commentary";
  }
  return "system";
}

function timelineIconClass(role: ReturnType<typeof timelineItemRole>) {
  const base = "relative z-[1] mt-1 flex size-6 items-center justify-center rounded-full bg-background";
  return role === "thinking" ? `${base} text-muted` : `${base} text-brand`;
}

function timelineTextCardClass(role: ReturnType<typeof timelineItemRole>) {
  const base = "min-w-0";
  if (role === "thinking") {
    return `${base} text-muted`;
  }
  return `${base} text-ink`;
}

function timelineProcessLabel({
  displayDuration,
  hasActiveTimeline,
  items,
  metrics,
  t,
}: {
  displayDuration: number | null;
  hasActiveTimeline: boolean;
  items: TimelineItem[];
  metrics: TimelineBlock["metrics"];
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const hasFailedItem = items.some((item) => item.status === "failed");
  const hasCancelledItem = items.some((item) => item.status === "cancelled");
  const state = hasActiveTimeline
    ? "processWorking"
    : metrics?.state === "completed"
      ? "processWorked"
      : metrics?.state === "failed" || hasFailedItem
        ? "processFailed"
        : hasCancelledItem
          ? "processStopped"
          : "processWorked";
  return displayDuration === null
    ? t(state)
    : t(`${state}Duration`, { duration: formatDuration(displayDuration) });
}

function TimelineItemIcon({ item }: { item: TimelineItem }) {
  if (item.kind === "tool_call") {
    return <Wrench className="shrink-0" size={13} />;
  }
  if (item.kind === "reasoning") {
    return <Brain className="shrink-0" size={13} />;
  }
  if (item.kind === "context_compaction") {
    if (item.status === "running") {
      return <LoaderCircle className="shrink-0 motion-safe:animate-spin" size={13} />;
    }
    if (item.status === "cancelled") {
      return <CircleStop className="shrink-0 text-muted" size={13} />;
    }
    if (item.status === "failed" || item.stage === "failed") {
      return <TriangleAlert className="shrink-0 text-danger" size={13} />;
    }
    return <Check className="shrink-0" size={13} />;
  }
  if (item.kind === "assistant_text" && (item.phase === "commentary" || item.phase === "pending")) {
    return <MessageCircle className="shrink-0" size={13} />;
  }
  return <Activity className="shrink-0" size={13} />;
}

function TimelineTextContent({
  item,
  markdown = false,
  role,
}: {
  item: TimelineTextItem;
  markdown?: boolean;
  role: ReturnType<typeof timelineItemRole> | "final-answer";
}) {
  const text = item.kind === "system_notice" && item.notice ? systemNoticeMessage(item.notice) : item.text;
  if (!text.trim()) {
    return <div className="text-sm leading-6 text-muted">…</div>;
  }
  if (markdown || item.phase === "final_answer") {
    return (
      <MarkdownRenderer
        className="markdown-body prose prose-sm max-w-none prose-headings:font-display prose-headings:tracking-[-0.035em] prose-p:my-2 prose-p:leading-[1.9] prose-ul:my-3 prose-ol:my-3 prose-li:my-1 prose-li:leading-[1.8]"
        text={text}
      />
    );
  }
  return (
    <p
      className={`min-w-0 whitespace-pre-wrap break-words text-sm leading-6 ${role === "thinking" ? "text-muted" : "text-ink/[0.82]"}`}
      data-timeline-text="true"
    >
      {text}
    </p>
  );
}

function FinalAnswerTextContent({ item }: { item: TimelineTextItem }) {
  const [visibleText, setVisibleText] = useState(() => item.replayFinalAnswer ? "" : item.text);

  useEffect(() => {
    if (!item.replayFinalAnswer) {
      setVisibleText(item.text);
      return;
    }
    if (visibleText.length >= item.text.length) {
      return;
    }
    const timer = window.setTimeout(() => {
      setVisibleText((current) =>
        item.text.slice(0, Math.min(current.length + FINAL_ANSWER_REPLAY_CHUNK_SIZE, item.text.length)),
      );
    }, FINAL_ANSWER_REPLAY_INTERVAL_MS);
    return () => window.clearTimeout(timer);
  }, [item.replayFinalAnswer, item.text, visibleText]);

  return <TimelineTextContent item={{ ...item, text: visibleText }} markdown role="final-answer" />;
}

function timelineItemTitle(item: TimelineItem) {
  if (item.kind === "tool_call") {
    return item.toolName;
  }
  if (item.kind === "reasoning") {
    return "Thinking";
  }
  if (item.kind === "execution_summary") {
    return "Execution summary";
  }
  if (item.kind === "context_compaction") {
    return "Context status";
  }
  if (item.kind === "assistant_text") {
    if (item.phase === "commentary" || item.phase === "pending") {
      return "Commentary";
    }
    if (item.phase === "final_answer") {
      return "Final answer";
    }
    return "Commentary";
  }
  if (item.phase === "system") {
    return item.title || "System";
  }
  return item.title || item.kind.replaceAll("_", " ");
}

function timelineItemKey(timelineId: string, itemId: string) {
  return `${timelineId}:${itemId}`;
}

function timelineItemStatus(item: TimelineItem) {
  const duration = timelineItemDuration(item);
  return duration === null
    ? item.status.replaceAll("_", " ")
    : `${item.status.replaceAll("_", " ")} · ${formatDuration(duration)}`;
}

function visibleTimelineItemStatus(item: TimelineItem) {
  if (item.kind === "tool_call") {
    const duration = timelineItemDuration(item);
    return duration === null ? undefined : formatDuration(duration);
  }
  if (item.status === "failed" || item.status === "cancelled") {
    return timelineItemStatus(item);
  }
  return undefined;
}

function timelineItemDuration(item: TimelineItem) {
  if (!item.completedAtMs) {
    return null;
  }
  return Math.max(0, item.completedAtMs - item.startedAtMs);
}

function timelineDuration(items: TimelineItem[], nowMs: number) {
  const started = items.map((item) => item.startedAtMs).filter((value) => value > 0);
  const completed = items
    .map((item) => item.completedAtMs ?? (item.status === "running" ? nowMs : null))
    .filter((value): value is number => value !== null && value > 0);
  if (started.length === 0 || completed.length === 0) {
    return null;
  }
  return Math.max(0, Math.max(...completed) - Math.min(...started));
}

function timelineToolStatus(item: TimelineItem) {
  if (item.kind !== "tool_call") {
    return "queued";
  }
  if (item.status === "queued") {
    return "queued";
  }
  if (item.status === "completed") {
    return item.isError ? "error" : "done";
  }
  if (item.status === "failed") {
    return "error";
  }
  if (item.status === "cancelled") {
    return "cancelled";
  }
  if (item.status === "waitingForInput") {
    return "waiting_dynamic_input";
  }
  return "running";
}

function formatArguments(value: unknown) {
  if (typeof value === "string") {
    return value;
  }
  return JSON.stringify(value, null, 2);
}
