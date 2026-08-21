import { useTranslation } from "react-i18next";

import type { ChatTimelineItem } from "@/shared/types/ui";

type ContextCompactionItem = Extract<ChatTimelineItem, { kind: "context_compaction" }>;

const TOKEN_FORMATTER = new Intl.NumberFormat("en-US", {
  maximumFractionDigits: 1,
  notation: "compact",
});

/** Renders automatic context maintenance in its owning Turn Timeline. */
export function ContextCompactionTimelineItem({ item }: { item: ContextCompactionItem }) {
  const { t } = useTranslation("chat");
  const workingContext = item.trigger === "working_context_exceeded";
  const terminal = item.status !== "running" || item.stage === "completed" || item.stage === "failed";
  const copy = terminal
    ? terminalCopy(item, workingContext, t)
    : {
        detail: t(`contextCompactionTimeline.${workingContext ? "working" : "automatic"}StageDetail.${item.stage}`),
        title: t(`contextCompactionTimeline.${workingContext ? "working" : "automatic"}Title`),
      };
  const metrics = terminal ? tokenTransition(item) : undefined;
  const meta = metrics
    ? t("contextCompactionTimeline.metrics", metrics)
    : !terminal || item.status === "failed" || item.stage === "failed"
      ? t(`commandCompactStageLabel.${item.stage}`)
      : undefined;

  return (
    <article className="min-w-0 px-1 py-1.5" data-context-compaction="timeline">
      <header className="flex min-w-0 items-center justify-between gap-3">
        <h4 className="truncate text-[12px] font-semibold tracking-[0.01em] text-ink">
          {copy.title}
        </h4>
        {meta ? <span className="shrink-0 text-[11px] text-muted">{meta}</span> : null}
      </header>
      <p className="mt-1 whitespace-pre-wrap break-words text-[13px] leading-5 text-muted">
        {copy.detail}
      </p>
    </article>
  );
}

function terminalCopy(
  item: ContextCompactionItem,
  workingContext: boolean,
  t: ReturnType<typeof useTranslation>["t"],
) {
  const outcome = item.status === "cancelled"
    ? "cancelled"
    : item.outcome ?? (item.status === "failed" || item.stage === "failed" ? "failed" : "applied");
  const group = workingContext ? "workingOutcome" : "automaticOutcome";
  return {
    detail: t(`contextCompactionTimeline.${group}.${outcome}.detail`),
    title: t(`contextCompactionTimeline.${group}.${outcome}.title`),
  };
}

function tokenTransition(item: ContextCompactionItem) {
  const before = item.metrics?.pre_compaction_total_tokens ?? 0;
  const after = item.metrics?.post_compaction_total_tokens ?? 0;
  return before > 0 && after > 0
    ? { after: TOKEN_FORMATTER.format(after), before: TOKEN_FORMATTER.format(before) }
    : undefined;
}
