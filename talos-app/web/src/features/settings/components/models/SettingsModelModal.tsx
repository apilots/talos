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

  return (
    <Dialog onOpenChange={(open) => { if (!open) onClose(); }} open>
      <DialogContent onCloseAutoFocus={returnFocus}>
        <DialogTitle className="pr-9 text-lg font-semibold">{isAdd ? t("modelsPanel.addModelTitle") : t("modelsPanel.editModelTitle")}</DialogTitle>
        <DialogDescription className="mt-1 text-sm text-muted">{isAdd ? t("modelsPanel.addModelDetail") : t("modelsPanel.editModelDetail")}</DialogDescription>
        <div className="mt-5 grid grid-cols-2 gap-4">
          <div className="col-span-2">
            <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.modelId")}</label>
            <Input
              disabled={!isAdd}
              onChange={(event) => onChange({ ...form, id: event.target.value })}
              placeholder="claude-3-5-sonnet"
              value={form.id}
            />
          </div>
          <div className="col-span-2">
            <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.modelName")}</label>
            <Input
              onChange={(event) => onChange({ ...form, name: event.target.value })}
              placeholder="Claude 3.5 Sonnet"
              value={form.name}
            />
          </div>
          <div className="col-span-2">
            <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.contextWindow")}</label>
            <Input
              onChange={(event) => onChange({ ...form, context_window: event.target.value })}
              value={form.context_window}
            />
          </div>
          <div>
            <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.reasoningEffort")}</label>
            <Select aria-label={t("modelsPanel.reasoningEffort")} disabled={!form.reasoning} onValueChange={(value) => onChange({ ...form, reasoning_effort: value as ReasoningEffort })} options={REASONING_EFFORTS.map((value) => ({ value, label: t(`modelsPanel.reasoningEffort_${value}`) }))} value={form.reasoning_effort} />
          </div>
          <div>
            <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.speed")}</label>
            <Select aria-label={t("modelsPanel.speed")} onValueChange={(value) => onChange({ ...form, speed: value as ModelSpeed })} options={MODEL_SPEEDS.map((value) => ({ value, label: t(`modelsPanel.speed_${value}`) }))} value={form.speed} />
          </div>
          <div className="col-span-2">
            <div className="mb-2 text-sm font-medium text-ink">{t("modelsPanel.caps")}</div>
            <div className="flex flex-wrap gap-2">
              <label className={["flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm font-medium transition select-none", form.reasoning ? `${selectionClasses.selected} ${selectionClasses.border}` : `border-border/60 bg-background/40 hover:border-selectedBorder ${selectionClasses.idle}`].join(" ")}>
                <input
                  checked={form.reasoning}
                  className="hidden"
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
                  <label key={kind} className={["flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm font-medium transition select-none", checked ? `${selectionClasses.selected} ${selectionClasses.border}` : `border-border/60 bg-background/40 hover:border-selectedBorder ${selectionClasses.idle}`].join(" ")}>
                    <input
                      checked={checked}
                      className="hidden"
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
          {allowReasoningReplay && <ReasoningReplaySelect value={form.reasoning_replay} onChange={(reasoning_replay) => onChange({ ...form, reasoning_replay })} />}
        </RequestParametersEditor>
        {error ? (
          <div className="mt-4 rounded-[1rem] border border-danger/20 bg-danger/[0.08] px-4 py-3 text-sm text-danger" role="alert">
            {error}
          </div>
        ) : null}
        <div className="mt-6 flex justify-end gap-3">
          <DialogClose asChild><Button type="button" variant="ghost">{t("modelsPanel.cancel")}</Button></DialogClose>
          <Button disabled={saving || !form.id.trim()} onClick={onSave} type="button">
            {saving ? t("modelsPanel.saving") : t("modelsPanel.save")}
          </Button>
        </div>
        <DialogClose asChild><button aria-label={t("common:close")} className="absolute right-5 top-5 flex size-8 items-center justify-center rounded-full text-muted outline-none hover:bg-hover focus-visible:ring-2 focus-visible:ring-brand/20" type="button"><X aria-hidden size={18} /></button></DialogClose>
      </DialogContent>
    </Dialog>
  );
}
