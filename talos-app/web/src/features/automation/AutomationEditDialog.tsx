import { useDialogFocusReturn } from "@/shared/ui/useDialogFocusReturn";
import { useState, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-browser";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/shared/ui/Dialog";
import { taskPrompt, type AutomationTask } from "./model";

export function AutomationEditDialog({ task, pending, error, onClose, onSave, fallbackFocusRef }: {
  fallbackFocusRef?: RefObject<HTMLElement | null>;
  task: AutomationTask; pending: boolean; error?: string; onClose: () => void;
  onSave: (action: AppProtocol.AutomationAction) => Promise<unknown>;
}) {
  const { t } = useTranslation(["automation", "common"]);
  const returnFocus = useDialogFocusReturn(fallbackFocusRef);
  const [prompt, setPrompt] = useState(() => taskPrompt(task));
  const [mode, setMode] = useState(() => task.type === "loop" ? task.value.schedule.type : "modelDecided");
  const [minutes, setMinutes] = useState(() => task.type === "loop" && task.value.schedule.type === "fixedInterval" ? String(task.value.schedule.minutes) : "5");
  const validInterval = mode !== "fixedInterval" || (Number.isInteger(Number(minutes)) && Number(minutes) >= 1 && Number(minutes) <= 4_294_967_295);
  const save = async () => {
    if (!prompt.trim() || !validInterval || pending) return;
    const action: AppProtocol.AutomationAction = task.type === "goal"
      ? { target: "goal", action: { type: "edit", goalId: task.value.id, expectedRevision: task.value.objectiveRevision, objective: prompt.trim() } }
      : { target: "loop", action: { type: "edit", loopId: task.value.id, expectedRevision: task.value.configRevision, prompt: prompt.trim(), schedule: mode === "fixedInterval" ? { type: "fixedInterval", minutes: Number(minutes) } : { type: "modelDecided" } } };
    try { await onSave(action); onClose(); } catch { /* The owning controller renders the typed failure. */ }
  };
  return <Dialog open onOpenChange={(open) => { if (!open && !pending) onClose(); }}>
    <DialogContent onCloseAutoFocus={returnFocus} className="max-w-lg p-6">
      <DialogTitle className="text-base font-medium text-ink">{t("automation:editTitle", { mode: task.type === "goal" ? "Goal" : "Loop" })}</DialogTitle>
      <DialogDescription className="mt-2 text-xs leading-5 text-muted">{t(task.type === "goal" ? "automation:goalEditHint" : "automation:loopEditHint")}</DialogDescription>
      <form className="mt-4 space-y-4" onSubmit={(event) => { event.preventDefault(); void save(); }}>
        <label className="block text-xs text-muted">{t("automation:prompt")}
          <textarea autoFocus className="form-control-focus mt-2 min-h-28 w-full resize-y rounded-xl border border-border bg-background px-3 py-2 text-sm text-ink" value={prompt} onChange={(event) => setPrompt(event.target.value)} disabled={pending} />
        </label>
        {task.type === "loop" ? <div className="flex flex-wrap items-end gap-3">
          <label className="flex-1 text-xs text-muted">{t("automation:schedule")}
            <select className="form-control-focus mt-2 w-full rounded-lg border border-border bg-card px-3 py-2 text-sm text-ink" value={mode} onChange={(event) => setMode(event.target.value as typeof mode)} disabled={pending}>
              <option value="modelDecided">{t("automation:modelDecided")}</option><option value="fixedInterval">{t("automation:fixedInterval")}</option>
            </select>
          </label>
          {mode === "fixedInterval" ? <label className="text-xs text-muted">{t("automation:minutes")}
            <input className="form-control-focus mt-2 block w-24 rounded-lg border border-border bg-card px-3 py-2 text-sm text-ink" type="number" min="1" max="4294967295" step="1" required value={minutes} onChange={(event) => setMinutes(event.target.value)} disabled={pending} />
          </label> : null}
        </div> : null}
        {error ? <p role="alert" className="text-xs text-danger">{error}</p> : null}
        <div className="flex justify-end gap-2">
          <button className="rounded-lg px-3 py-2 text-sm text-muted hover:bg-hover" disabled={pending} type="button" onClick={onClose}>{t("common:cancel")}</button>
          <button className="rounded-lg bg-ink px-3 py-2 text-sm text-card disabled:opacity-40" disabled={pending || !prompt.trim() || !validInterval} type="submit">{t("common:save")}</button>
        </div>
      </form>
    </DialogContent>
  </Dialog>;
}
