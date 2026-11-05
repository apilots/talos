import { AutomationModeIcon } from "@/shared/ui/AutomationModeIcon";
import { useTranslation } from "react-i18next";
import { AutomationTaskControls } from "./AutomationTaskControls";
import type { AppProtocol } from "@talos/sdk-browser";
import { useNowMs } from "@/shared/hooks/useNowMs";
import { formatDuration } from "@/shared/lib/formatDuration";
import { taskPrompt, type AutomationTask, type AutomationTurnTiming } from "./model";
import { initialTaskWait } from "./initialTaskWait";

export function AutomationTaskRow({ task, activeTurn, queuedLoop = false, pending = false, onAction, onEdit }: {
  task: AutomationTask; activeTurn?: AutomationTurnTiming; queuedLoop?: boolean; pending?: boolean;
  onAction: (action: AppProtocol.AutomationAction) => Promise<unknown>;
  onEdit: (task: AutomationTask) => void;
}) {
  const { t, i18n } = useTranslation("automation");
  const label = task.type === "goal" ? "Goal" : "Loop";
  const loopTurn = task.type === "loop" && activeTurn?.origin.type === "loop" && activeTurn.origin.loopId === task.value.id ? activeTurn : undefined;
  const initialWait = initialTaskWait(task, activeTurn, queuedLoop);
  const countdownDueAt = initialWait?.dueAt;
  const countingDown = countdownDueAt !== undefined && countdownDueAt !== null;
  const now = Math.floor(useNowMs(task.type === "goal" ? task.value.status !== "complete" : Boolean(loopTurn || countingDown)) / 1000);
  const remaining = initialWait?.dueAt === null || initialWait === undefined ? 0 : Math.max(0, initialWait.dueAt - now);
  const elapsed = initialWait ? undefined : task.type === "goal"
    ? Math.max(0, (task.value.status === "complete" ? task.value.updatedAt : now) - task.value.createdAt)
    : loopTurn ? Math.max(0, now - loopTurn.startedAt) : undefined;
  const duration = elapsed === undefined ? undefined : elapsed < 1 ? "0s" : formatDuration(Math.floor(elapsed) * 1000);
  const date = (seconds: number) => new Date(seconds * 1000).toLocaleString(i18n.resolvedLanguage, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
  const compactDate = (seconds: number) => new Date(seconds * 1000).toLocaleDateString(i18n.resolvedLanguage, { month: "numeric", day: "numeric" });
  const compactTime = (seconds: number) => new Date(seconds * 1000).toLocaleTimeString(i18n.resolvedLanguage, { hour: "2-digit", minute: "2-digit", hour12: false });
  const fullDate = (seconds: number) => new Date(seconds * 1000).toLocaleString(i18n.resolvedLanguage, { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit", timeZoneName: "short" });
  const nextDueAt = task.type === "loop" && task.value.status === "active" ? task.value.nextDueAt : null;
  const displayedDueAt = initialWait ? initialWait.dueAt : nextDueAt;
  const dateTime = duration ? `PT${Math.floor(elapsed!)}S` : displayedDueAt !== null ? new Date(displayedDueAt * 1000).toISOString() : undefined;
  return <div className="min-w-0 px-1.5 py-1" data-testid={`automation-task-${task.value.id}`}>
    <div className="flex min-h-9 items-center gap-1 sm:gap-2">
      <span className="flex shrink-0 items-center gap-1 text-xs text-muted sm:gap-1.5"><AutomationModeIcon mode={task.type} />{label}</span>
      <span className="min-w-0 flex-1 truncate text-[13px] text-ink/85" title={taskPrompt(task)}>{taskPrompt(task)}</span>
      <time className="shrink-0 whitespace-nowrap text-[11px] tabular-nums text-muted" dateTime={dateTime} title={initialWait ? undefined : duration ? t(task.type === "goal" ? "goalElapsed" : "loopElapsed") : nextDueAt !== null ? `${t("next")}: ${fullDate(nextDueAt)}` : undefined}>
        {initialWait ? <span className="block max-w-16 whitespace-normal text-right leading-3 sm:max-w-none sm:whitespace-nowrap">{remaining > 0 ? t("startsInSeconds", { seconds: remaining }) : t("waiting")}</span> : duration ?? (nextDueAt !== null ? <>
          <span className="hidden sm:inline">{date(nextDueAt)}</span>
          <span className="flex flex-col items-end leading-3 sm:hidden" data-testid="automation-next-time-compact"><span>{compactDate(nextDueAt)}</span><span>{compactTime(nextDueAt)}</span></span>
        </> : <span className="block max-w-14 whitespace-normal text-right leading-3 sm:max-w-none sm:whitespace-nowrap">{task.type === "loop" && task.value.status === "stopped" ? t("status.stopped") : task.type === "loop" && task.value.schedule.type === "modelDecided" ? t("modelDecided") : t("noNextTime")}</span>)}
      </time>
      <AutomationTaskControls task={task} cancelInitial={Boolean(initialWait)} pending={pending} onAction={onAction} onEdit={onEdit} />
    </div>
  </div>;
}
