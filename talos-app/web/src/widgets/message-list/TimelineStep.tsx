import type { ReactNode } from "react";
import { Activity, Brain, ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";

import { systemNoticeMessage } from "@/shared/i18n/systemNoticeMessage";
import type { ChatTimelineItem } from "@/shared/types/ui";
import { ContextCompactionTimelineItem } from "./context-compaction/ContextCompactionTimelineItem";
import { TimelineRailRow } from "./TimelineRailRow";
import { timelineToolStatus } from "./timelineDisplay";
import { timelineToolIcon } from "./timelineToolPresentation";
import { ToolCallBlock } from "./ToolCallBlock";

export function TimelineStep({
  item,
  itemKey,
  live = false,
  placement = "rail",
  onToolOpen,
  children,
}: {
  item: ChatTimelineItem;
  itemKey: string;
  live?: boolean;
  placement?: "rail" | "tool-group" | "thinking-group";
  onToolOpen?: () => void;
  children?: ReactNode;
}) {
  const { t } = useTranslation("chat");
  if (item.kind === "tool_call") {
    return (
      <TimelineRailRow icon={timelineToolIcon(item.toolName)}>
        <div
          data-process-step="true"
          data-timeline-item-key={itemKey}
          data-timeline-role="tool"
        >
          <ToolCallBlock
            argumentsValue={item.arguments}
            callId={item.callId}
            contentItems={item.contentItems}
            live={live}
            onOpen={onToolOpen}
            output={item.output}
            progress={item.progress}
            prompt={item.prompt}
            startedAtMs={item.startedAtMs}
            status={timelineToolStatus(item)}
            timingMs={item.executionMetrics?.executionDurationMs}
            toolName={item.toolName}
            variant={placement === "tool-group" ? "activity" : "timeline"}
          />
          {children}
        </div>
      </TimelineRailRow>
    );
  }
  const thinking = item.kind === "reasoning";
  const content = (
    <div data-process-step="true" data-timeline-item-key={itemKey} data-timeline-role={thinking ? "thinking" : "system"}>
      {item.kind === "context_compaction" ? <ContextCompactionTimelineItem item={item} /> : thinking && placement === "tool-group" ? (
        <details className="group/thinking">
          <summary className="flex w-fit cursor-pointer list-none items-center gap-2 rounded-md px-1 py-1.5 text-[13px] text-muted marker:hidden hover:text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand">
            {t("timelineThinkingDetails")}
            <ChevronDown aria-hidden="true" className="shrink-0 group-open/thinking:rotate-180" size={13} />
          </summary>
          <p className="whitespace-pre-wrap break-words px-1 py-1 text-sm leading-6 text-muted" data-timeline-text="true">{item.text}</p>
        </details>
      ) : (
        <article className="min-w-0 px-1 py-1" data-timeline-surface="plain">
          {item.kind === "system_notice" ? (
            <div className="text-xs font-medium text-ink">{t("systemNoticeTitle")}</div>
          ) : item.kind === "execution_summary" ? (
            <div className="text-xs font-medium text-ink">{t("timelineExecutionSummary")}</div>
          ) : null}
          <p className="whitespace-pre-wrap break-words text-sm leading-6 text-muted" data-timeline-text="true">
            {item.kind === "system_notice" && item.notice ? systemNoticeMessage(item.notice) : item.text}
          </p>
        </article>
      )}
    </div>
  );
  return thinking && placement === "thinking-group" ? content : (
    <TimelineRailRow icon={thinking ? Brain : Activity}>{content}</TimelineRailRow>
  );
}
