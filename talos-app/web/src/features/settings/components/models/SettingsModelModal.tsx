import { useId } from "react";
import { selectionClasses } from "@/shared/ui/controlStyles";
import type { ModelSpeed, ReasoningEffort } from "@talos/sdk-browser";
import { Brain, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ReasoningReplaySelect } from "./ReasoningReplaySelect";
import { RequestParametersEditor } from "./RequestParametersEditor";

import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Dialog, DialogClose, DialogContent, DialogTitle, DialogDescription } from "@/shared/ui/Dialog";
import { Select } from "@/shared/ui/Select";
import { useSettingsDialogFocus } from "./useSettingsDialogFocus";
import {
  INPUT_KIND_ICONS,
  INPUT_KINDS,
  MODEL_SPEEDS,
  REASONING_EFFORTS,
  type ModelFormState,
} from "./modelForm";

interface SettingsModelModalProps {
  mode: "add" | "edit";
  form: ModelFormState;
  saving: boolean;
  allowReasoningReplay?: boolean;
  error: string;
  onChange: (form: ModelFormState) => void;
  onSave: () => void;
  onClose: () => void;
}

export function SettingsModelModal({
  mode,
  form,
  saving,
  allowReasoningReplay = false,
  error,
  onChange,
  onSave,
  onClose,
}: SettingsModelModalProps) {
  const { t } = useTranslation("settings");
  const isAdd = mode === "add";
  const returnFocus = useSettingsDialogFocus();
  const formId = useId();
  const efforts = form.reasoning_efforts?.length ? REASONING_EFFORTS.filter((effort) => form.reasoning_efforts?.includes(effort)) : REASONING_EFFORTS;
  const validDefault = !form.reasoning || efforts.includes(form.reasoning_effort);

  return (
    <Dialog onOpenChange={(open) => { if (!open) onClose(); }} open>
      <DialogContent className="flex max-h-[calc(100dvh-2rem)] flex-col overflow-hidden p-0 sm:max-h-[calc(100dvh-5rem)]" onCloseAutoFocus={returnFocus}>
        <div className="shrink-0 px-6 pb-4 pt-6">
          <DialogTitle className="pr-9 text-lg font-semibold">{isAdd ? t("modelsPanel.addModelTitle") : t("modelsPanel.editModelTitle")}</DialogTitle>
          <DialogDescription className="mt-1 text-sm text-muted">{isAdd ? t("modelsPanel.addModelDetail") : t("modelsPanel.editModelDetail")}</DialogDescription>
        </div>
        <div className="min-h-0 overflow-y-auto overscroll-contain px-6 pb-5">
          <div className="grid grid-cols-2 gap-4">
            <div className="col-span-2">
              <label className="mb-1.5 block text-sm font-medium text-ink" htmlFor={`${formId}-id`}>{t("modelsPanel.modelId")}</label>
              <Input
                id={`${formId}-id`}
                autoComplete="off"
                spellCheck={false}
                disabled={!isAdd}
                onChange={(event) => onChange({ ...form, id: event.target.value })}
                placeholder="claude-3-5-sonnet"
                value={form.id}
              />
            </div>
            <div className="col-span-2">
              <label className="mb-1.5 block text-sm font-medium text-ink" htmlFor={`${formId}-name`}>{t("modelsPanel.modelName")}</label>
              <Input
                id={`${formId}-name`}
                autoComplete="off"
                onChange={(event) => onChange({ ...form, name: event.target.value })}
                placeholder="Claude 3.5 Sonnet"
                value={form.name}
              />
            </div>
            <div className="col-span-2">
              <label className="mb-1.5 block text-sm font-medium text-ink" htmlFor={`${formId}-context`}>{t("modelsPanel.contextWindow")}</label>
              <Input
                id={`${formId}-context`}
                inputMode="numeric"
                autoComplete="off"
                onChange={(event) => onChange({ ...form, context_window: event.target.value })}
                value={form.context_window}
              />
            </div>
            <div>
              <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.reasoningEffort")}</label>
              <Select aria-label={t("modelsPanel.reasoningEffort")} disabled={!form.reasoning} onValueChange={(value) => onChange({ ...form, reasoning_effort: value as ReasoningEffort })} options={efforts.map((value) => ({ value, label: t(`modelsPanel.reasoningEffort_${value}`) }))} value={form.reasoning_effort} />
              {!validDefault && <p className="mt-1.5 text-xs text-danger" role="alert">{t("modelsPanel.reasoningDefaultUnsupported")}</p>}
            </div>
            <div>
              <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.speed")}</label>
              <Select aria-label={t("modelsPanel.speed")} onValueChange={(value) => onChange({ ...form, speed: value as ModelSpeed })} options={MODEL_SPEEDS.map((value) => ({ value, label: t(`modelsPanel.speed_${value}`) }))} value={form.speed} />
            </div>
            <div className="col-span-2">
              <div className="mb-2 text-sm font-medium text-ink">{t("modelsPanel.caps")}</div>
              <div className="flex flex-wrap gap-2">
                <label className={["flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm font-medium transition select-none focus-within:ring-2 focus-within:ring-brand/20", form.reasoning ? `${selectionClasses.selected} ${selectionClasses.border}` : `border-border/60 bg-background/40 hover:border-selectedBorder ${selectionClasses.idle}`].join(" ")}>
                  <input
                    checked={form.reasoning}
                    className="sr-only"
                    onChange={(event) => onChange({ ...form, reasoning: event.target.checked })}
                    type="checkbox"
                  />
                  <Brain size={13} />
                  {t("modelsPanel.capReasoning")}
                </label>
                {INPUT_KINDS.map((kind) => {
                  const checked = form.input.includes(kind);
                  const KindIcon = INPUT_KIND_ICONS[kind];
                  return (
                    <label key={kind} className={["flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm font-medium transition select-none focus-within:ring-2 focus-within:ring-brand/20", checked ? `${selectionClasses.selected} ${selectionClasses.border}` : `border-border/60 bg-background/40 hover:border-selectedBorder ${selectionClasses.idle}`].join(" ")}>
                      <input
                        checked={checked}
                        className="sr-only"
                        onChange={(event) => {
                          const next = event.target.checked
                            ? [...form.input, kind]
                            : form.input.filter((entry) => entry !== kind);
                          onChange({ ...form, input: next });
                        }}
                        type="checkbox"
                      />
                      <KindIcon size={13} />
                      {kind}
                    </label>
                  );
                })}
              </div>
            </div>
          </div>
          <RequestParametersEditor rows={form.request_parameters} onChange={(request_parameters) => onChange({ ...form, request_parameters })}>
            <fieldset className="mt-4 space-y-3 rounded-xl border border-border/70 bg-background/40 p-3 text-sm" disabled={!form.reasoning}>
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={form.reasoning_efforts !== undefined} onChange={(event) => onChange({ ...form, reasoning_efforts: event.target.checked ? [...REASONING_EFFORTS] : undefined })} />
                {t("modelsPanel.restrictReasoningEfforts")}
              </label>
              <p className="text-xs text-muted">{t("modelsPanel.reasoningEffortsHint")}</p>
              {form.reasoning_efforts !== undefined && <div className="flex flex-wrap gap-2">{REASONING_EFFORTS.map((effort) => <label key={effort} className="flex cursor-pointer items-center gap-2 rounded-lg border border-border/70 bg-card px-2.5 py-1.5 focus-within:ring-2 focus-within:ring-brand/20">
                <input type="checkbox" checked={form.reasoning_efforts?.includes(effort) ?? false} onChange={(event) => onChange({ ...form, reasoning_efforts: REASONING_EFFORTS.filter((value) => value === effort ? event.target.checked : form.reasoning_efforts?.includes(value)) })} />
                {t(`modelsPanel.reasoningEffort_${effort}`)}
              </label>)}</div>}
            </fieldset>
            {allowReasoningReplay && <ReasoningReplaySelect value={form.reasoning_replay} onChange={(reasoning_replay) => onChange({ ...form, reasoning_replay })} />}
          </RequestParametersEditor>
          {error ? (
            <div className="mt-4 rounded-[1rem] border border-danger/20 bg-danger/[0.08] px-4 py-3 text-sm text-danger" role="alert">
              {error}
            </div>
          ) : null}
        </div>
        <div className="flex shrink-0 justify-end gap-3 border-t border-border/60 bg-card px-6 py-4">
          <DialogClose asChild><Button type="button" variant="ghost">{t("modelsPanel.cancel")}</Button></DialogClose>
          <Button disabled={saving || !form.id.trim() || !validDefault} onClick={onSave} type="button">
            {saving ? t("modelsPanel.saving") : t("modelsPanel.save")}
          </Button>
        </div>
        <DialogClose asChild><button aria-label={t("common:close")} className="absolute right-5 top-5 flex size-8 items-center justify-center rounded-full text-muted outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-brand/20" type="button"><X aria-hidden size={18} /></button></DialogClose>
      </DialogContent>
    </Dialog>
  );
}
