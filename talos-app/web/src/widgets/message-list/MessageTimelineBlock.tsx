import type { ReactNode } from "react";
import type { PendingUserAction } from "@talos/sdk-browser";
import { LoaderCircle, MessageCircle, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { apiErrorMessage } from "@/shared/i18n/apiErrorMessage";
import type { ChatRenderBlock, ChatTimelineItem } from "@/shared/types/ui";
import { Button } from "@/shared/ui/Button";
import { CopyableBlock } from "./CopyableBlock";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { TimelineActivityGroup } from "./TimelineActivityGroup";
import { TimelineProcessHeader } from "./TimelineProcessHeader";
import { TimelineRailRow } from "./TimelineRailRow";
import { TimelineStep } from "./TimelineStep";
import { TurnMetricsFooter } from "./TurnMetricsFooter";
import { groupTimelineItems, TIMELINE_PAGE_SIZE, timelineProcessTiming } from "./timelineDisplay";

type TimelineBlock = Extract<ChatRenderBlock, { kind: "timeline" }>;
type TimelineTextItem = Exclude<ChatTimelineItem, { kind: "tool_call" }>;

interface MessageTimelineBlockProps {
  block: TimelineBlock;
  copiedLabel: string;
  copyLabel: string;
  helpfulLabel: string;
  notHelpfulLabel: string;
  pendingUserAction?: PendingUserAction;
  renderUserAction?: (action: PendingUserAction) => ReactNode;
  showWaitingIndicator?: boolean;
  liveUpdatesPaused?: boolean;
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
  showWaitingIndicator = true,
  liveUpdatesPaused = false,
}: MessageTimelineBlockProps) {
  const { t } = useTranslation("chat");
  const items = block.items.filter(
    (item) => item.kind !== "context_compaction" || item.trigger !== "user_requested",
  );
  const answers = items.filter(isFinalAnswer);
  const processItems = items.filter((item) => !isFinalAnswer(item));
  const [inspectedTools, setInspectedTools] = useState<ReadonlySet<string>>(() => new Set());
  const rows = groupTimelineItems(processItems, inspectedTools);
  const [processExpanded, setProcessExpanded] = useState<boolean>();
  const [rowLimit, setRowLimit] = useState(TIMELINE_PAGE_SIZE);
  const activeItems = processItems.filter(
    (item) => item.status === "running" || item.status === "queued" || item.status === "waitingForInput",
  );
  const active = block.live ?? (activeItems.length > 0);
  const live = active && !liveUpdatesPaused;
  const expanded = processExpanded ?? (active || inspectedTools.size > 0);
  const inputRows = rows.filter((row) => row.kind === "tool_call" && row.status === "waitingForInput");
  const firstRecentRow = Math.max(0, rows.length - rowLimit);
  const latestCommentary = firstRecentRow > 0 ? lastCommentary(processItems) : undefined;
  const activeTools = new Set(activeItems.filter((item) => item.kind === "tool_call"));
  const visibleRows = rows.filter((row, index) => {
    const tool = row.kind === "tool_call" ? row : undefined;
    if (tool && inspectedTools.has(tool.itemId)) return true;
    return expanded
      ? index >= firstRecentRow || row === latestCommentary || (tool && activeTools.has(tool))
      : tool?.status === "waitingForInput";
  });
  const showEarlier = expanded && visibleRows.length < rows.length;
  const showWorkingIndicator = expanded && live && showWaitingIndicator && answers.length === 0 &&
    inputRows.length === 0 && rows.length > 0;
  const waitingForUpdate = activeItems.every((item) => item.kind === "reasoning");

  if (!block.error && items.length === 0) {
    return null;
  }

  return (
    <div className="space-y-2">
      {rows.length > 0 ? (
        <div>
          <TimelineProcessHeader
            expanded={expanded}
            live={active}
            onToggle={inputRows.length < rows.length ? () => {
              if (expanded) setInspectedTools(new Set());
              setProcessExpanded(!expanded);
            } : undefined}
            paused={liveUpdatesPaused}
            timing={timelineProcessTiming(processItems, block)}
          />
          {visibleRows.length || showWorkingIndicator ? (
            <div className="space-y-2 border-t border-border/35 px-2 pt-2.5 md:px-3" data-process-surface="rail">
              {showEarlier ? (
                <Button type="button" variant="ghost" onClick={() => setRowLimit((count) => count + TIMELINE_PAGE_SIZE)}>
                  {t("timelineShowEarlierActivity")}
                </Button>
              ) : null}
              {visibleRows.map((row) => {
                const itemKey = `${block.id}:${row.itemId}`;
                if (row.kind === "activity") {
                  return <TimelineActivityGroup group={row} key={itemKey} timelineId={block.id} />;
                }
                if (row.kind === "assistant_text") {
                  return row.text.trim() ? (
                    <TimelineRailRow icon={MessageCircle} key={itemKey}>
                      <p
                        className="min-w-0 whitespace-pre-wrap break-words px-1 py-1 text-sm leading-6 text-ink"
                        data-timeline-item-key={itemKey}
                        data-timeline-role="commentary"
                      >
                        {row.text}
                      </p>
                    </TimelineRailRow>
                  ) : null;
                }
                return (
                  <TimelineStep
                    item={row} itemKey={itemKey} key={itemKey} live={live}
                    onToolOpen={row.kind === "tool_call" && activeTools.has(row) ? () => {
                      // Keep the inspected row mounted on settlement without opening a collapsed rail.
                      setInspectedTools((current) => current.has(row.itemId) ? current : new Set([...current, row.itemId]));
                    } : undefined}
                  >
                    {pendingUserAction?.itemId === row.itemId ? renderUserAction?.(pendingUserAction) : null}
                  </TimelineStep>
                );
              })}
              {showWorkingIndicator ? (
                <TimelineRailRow icon={LoaderCircle} spinning>
                  <p className={waitingForUpdate ? "px-1 py-1.5 text-[13px] text-muted" : "sr-only"} role="status">
                    {waitingForUpdate
                      ? t(activeItems.length ? "timelineThinking" : "waitingForTimelineUpdate")
                      : t("processWorking")}
                  </p>
                </TimelineRailRow>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : null}
      {answers.map((item, index) => {
        const itemKey = `${block.id}:${item.itemId}`;
        return (
          <CopyableBlock
            copiedLabel={copiedLabel}
            copyLabel={copyLabel}
            copyText={item.text}
            helpfulLabel={helpfulLabel}
            key={itemKey}
            notHelpfulLabel={notHelpfulLabel}
            variant="response"
          >
            <section
              className="min-w-0 px-1 py-2 text-ink"
              data-response-surface="plain"
              data-timeline-item-key={itemKey}
              data-timeline-response="true"
            >
              <FinalAnswerTextContent item={item} />
              {live && item.status === "running" ? (
                <span
                  aria-label={t("streamingFinalAnswer")}
                  className="mt-2 inline-block size-1.5 rounded-full bg-brand motion-safe:animate-pulse"
                />
              ) : null}
              {block.metrics && index === answers.length - 1 ? <TurnMetricsFooter metrics={block.metrics} /> : null}
            </section>
          </CopyableBlock>
        );
      })}
      {block.error ? (
        <div role="alert" className="flex items-start gap-2 px-1 py-2 text-sm leading-6 text-danger">
          <TriangleAlert aria-hidden="true" className="mt-1 shrink-0" size={14} />
          <p className="min-w-0 whitespace-pre-wrap break-words">{apiErrorMessage(block.error)}</p>
        </div>
      ) : null}
    </div>
  );
}

function lastCommentary(items: ChatTimelineItem[]) {
  for (let index = items.length - 1; index >= 0; index--) {
    const item = items[index];
    if (item.kind === "assistant_text" && item.text.trim()) return item;
  }
  return undefined;
}

function isFinalAnswer(item: ChatTimelineItem): item is TimelineTextItem & { kind: "assistant_text"; phase: "final_answer" } {
  return item.kind === "assistant_text" && item.phase === "final_answer";
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

  return visibleText.trim() ? (
    <MarkdownRenderer
      className="markdown-body prose prose-sm max-w-none prose-headings:font-display prose-headings:tracking-[-0.035em] prose-p:my-2 prose-p:leading-[1.9] prose-ul:my-3 prose-ol:my-3 prose-li:my-1 prose-li:leading-[1.8]"
      text={visibleText}
    />
  ) : <div className="text-sm leading-6 text-muted">…</div>;
}
