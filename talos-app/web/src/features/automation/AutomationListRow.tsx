import { ArrowUpRight } from "lucide-react";
import { Link } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useNowMs } from "@/shared/hooks/useNowMs";
import { formatDuration } from "@/shared/lib/formatDuration";
import { AutomationModeIcon } from "@/shared/ui/AutomationModeIcon";
import { taskPrompt } from "./model";
import { AutomationTaskControls, type AutomationTaskControlsProps } from "./AutomationTaskControls";

/** A summary and one metadata line; execution details stay in the original chat. */
export function AutomationListRow({ task, ...controls }: AutomationTaskControlsProps) {
  const { t, i18n } = useTranslation("automation");
  const prompt = taskPrompt(task);
  const label = task.type === "goal" ? "Goal" : "Loop";
  const now = Math.floor(useNowMs(task.type === "goal" && task.value.status !== "complete") / 1000);
  let timing: string;
  if (task.type === "goal") {
    const elapsed = Math.max(0, (task.value.status === "complete" ? task.value.updatedAt : now) - task.value.createdAt);
    timing = t("listElapsed", { duration: elapsed < 1 ? "0s" : formatDuration(elapsed * 1000) });
  } else {
    const schedule = task.value.schedule.type === "fixedInterval" ? t("everyMinutes", { count: task.value.schedule.minutes }) : t("modelDecided");
    const next = task.value.status === "active" ? task.value.nextDueAt : null;
    timing = next === null ? schedule : `${schedule} · ${t("next")} ${new Date(next * 1000).toLocaleString(i18n.resolvedLanguage, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}`;
  }
  const reasonKind = task.type === "goal"
    ? task.value.status === "paused" && task.value.pauseReason !== "userPaused" ? task.value.pauseReason : null
    : task.value.status === "stopped" && task.value.stopReason !== "userStopped" ? task.value.stopReason : null;
  const reason = reasonKind ? task.value.reason?.trim().replace(/\s+/g, " ") || t(`listReasons.${reasonKind}`) : undefined;
  const status = t(`status.${task.value.status}`);
  const metadata = [label, timing, status, reason].filter(Boolean).join(" · ");
  return <article className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1.5 py-5 sm:gap-x-5" data-testid={`automation-list-task-${task.value.id}`}>
    <div className="col-span-2 flex min-w-0 items-center gap-2.5 sm:col-span-1">
      <AutomationModeIcon mode={task.type} className="shrink-0 text-muted" />
      <h2 className="min-w-0 flex-1 truncate text-[15px] font-medium leading-6 text-ink" title={prompt}>{prompt}</h2>
    </div>
    <p className="col-start-1 row-start-2 flex min-w-0 items-center truncate pl-[25px] text-xs leading-5 text-muted" title={metadata} data-testid="automation-list-metadata">
      <span className="min-w-0 truncate">{label} · {timing}</span>
      <span aria-hidden="true" className="mx-1 shrink-0"> · </span>
      <span className="shrink-0" data-testid="automation-list-status">{status}</span>
      {reason ? <span className="min-w-0 truncate"> · {reason}</span> : null}
    </p>
    <div className="col-start-2 row-start-2 flex shrink-0 items-center justify-end gap-1 sm:row-span-2 sm:row-start-1" data-testid="automation-list-actions">
      <AutomationTaskControls task={task} {...controls} />
      <Link className="shrink-0 rounded p-1 text-muted transition hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/25" to={`/sessions/${encodeURIComponent(task.value.sessionId)}`} aria-label={`${t("openSession")}: ${prompt}`} title={t("openSession")}><ArrowUpRight size={15} strokeWidth={1.3} aria-hidden="true" /></Link>
    </div>
  </article>;
}
