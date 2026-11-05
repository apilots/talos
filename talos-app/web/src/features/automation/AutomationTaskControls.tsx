import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-browser";
import { AutomationActionIcon } from "@/shared/ui/AutomationModeIcon";
import { X } from "lucide-react";
import { taskCanEdit, type AutomationTask } from "./model";

export interface AutomationTaskControlsProps {
  task: AutomationTask;
  pending?: boolean;
  cancelInitial?: boolean;
  onAction: (action: AppProtocol.AutomationAction) => Promise<unknown>;
  onEdit: (task: AutomationTask) => void;
}

/** Keep the same typed actions and focus owner in composer and management rows. */
export function AutomationTaskControls({ task, pending = false, cancelInitial = false, onAction, onEdit }: AutomationTaskControlsProps) {
  const { t } = useTranslation("automation");
  const label = task.type === "goal" ? "Goal" : "Loop";
  const action = (type: "pause" | "resume" | "stop" | "clear") => {
    const value: AppProtocol.AutomationAction = task.type === "goal"
      ? { target: "goal", action: { type: type === "stop" ? "clear" : type, goalId: task.value.id } }
      : { target: "loop", action: { type: type === "stop" ? "stop" : "clear", loopId: task.value.id } };
    void onAction(value).catch(() => undefined);
  };
  const controls = "shrink-0 rounded p-1 text-muted transition hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/25 disabled:opacity-40";
  return <>
    {task.type === "goal" && (task.value.status === "active" || task.value.status === "paused") ? <button aria-label={t(task.value.status === "active" ? "pauseGoal" : "resumeGoal")} title={t(task.value.status === "active" ? "pauseGoal" : "resumeGoal")} className={controls} disabled={pending} type="button" onClick={() => action(task.value.status === "active" ? "pause" : "resume")}><AutomationActionIcon action={task.value.status === "active" ? "pause" : "resume"} /></button> : null}
    {task.type === "loop" && task.value.status === "active" ? <button aria-label={t("stopLoop")} title={t("stopLoop")} className={controls} disabled={pending} type="button" onClick={() => action("stop")}><AutomationActionIcon action="stop" /></button> : null}
    <button aria-label={t(cancelInitial ? "cancelTitle" : "clearTitle", { mode: label })} title={t(cancelInitial ? "cancelTitle" : "clearTitle", { mode: label })} className={controls} disabled={pending} type="button" onClick={() => action("clear")}>{cancelInitial ? <X size={14} strokeWidth={1.3} aria-hidden="true" /> : <AutomationActionIcon action="clear" />}</button>
    {taskCanEdit(task) ? <button aria-label={t("editTitle", { mode: label })} title={t("editTitle", { mode: label })} className={controls} disabled={pending} type="button" onClick={(event) => { event.currentTarget.focus(); onEdit(task); }}><AutomationActionIcon action="edit" /></button> : null}
  </>;
}
