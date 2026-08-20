import type { ModelSpeed, ReasoningEffort } from "@talos/sdk-browser";
import { Brain, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
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
  error: string;
  onChange: (form: ModelFormState) => void;
  onSave: () => void;
  onClose: () => void;
}

export function SettingsModelModal({
  mode,
  form,
  saving,
  error,
  onChange,
  onSave,
  onClose,
}: SettingsModelModalProps) {
  const { t } = useTranslation("settings");
  const isAdd = mode === "add";

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 px-6 py-10 backdrop-blur-[2px]">
      <div className="absolute inset-0" onClick={onClose} />
      <Panel className="relative z-10 w-full max-w-[520px] rounded-[var(--radius-panel)] border-border bg-card p-6 shadow-panel">
        <div className="flex items-start justify-between gap-6">
          <div>
            <div className="text-lg font-semibold text-ink">
              {isAdd ? t("modelsPanel.addModelTitle") : t("modelsPanel.editModelTitle")}
            </div>
            <div className="mt-1 text-sm text-muted">
              {isAdd ? t("modelsPanel.addModelDetail") : t("modelsPanel.editModelDetail")}
            </div>
          </div>
          <button
            className="inline-flex h-10 w-10 items-center justify-center rounded-full text-muted transition hover:bg-surface hover:text-ink"
            onClick={onClose}
            type="button"
          >
            <X size={18} />
          </button>
        </div>
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
            <select
              className="w-full rounded-lg border border-border/70 bg-card px-3 py-2 text-sm text-ink disabled:opacity-50"
              disabled={!form.reasoning}
              onChange={(event) =>
                onChange({ ...form, reasoning_effort: event.target.value as ReasoningEffort })
              }
              value={form.reasoning_effort}
            >
              {REASONING_EFFORTS.map((effort) => (
                <option key={effort} value={effort}>
                  {t(`modelsPanel.reasoningEffort_${effort}`)}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="mb-1.5 block text-sm font-medium text-ink">{t("modelsPanel.speed")}</label>
            <select
              className="w-full rounded-lg border border-border/70 bg-card px-3 py-2 text-sm text-ink"
              onChange={(event) => onChange({ ...form, speed: event.target.value as ModelSpeed })}
              value={form.speed}
            >
              {MODEL_SPEEDS.map((speed) => (
                <option key={speed} value={speed}>
                  {t(`modelsPanel.speed_${speed}`)}
                </option>
              ))}
            </select>
          </div>
          <div className="col-span-2">
            <div className="mb-2 text-sm font-medium text-ink">{t("modelsPanel.caps")}</div>
            <div className="flex flex-wrap gap-2">
              <label className={["flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm font-medium transition select-none", form.reasoning ? "border-ink bg-ink text-background" : "border-border/60 bg-background/40 text-muted hover:border-ink/40 hover:text-ink"].join(" ")}>
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
                  <label key={kind} className={["flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm font-medium transition select-none", checked ? "border-ink bg-ink text-background" : "border-border/60 bg-background/40 text-muted hover:border-ink/40 hover:text-ink"].join(" ")}>
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
        {error ? (
          <div className="mt-4 rounded-[1rem] border border-danger/20 bg-danger/8 px-4 py-3 text-sm text-danger">
            {error}
          </div>
        ) : null}
        <div className="mt-6 flex justify-end gap-3">
          <Button onClick={onClose} type="button" variant="ghost">
            {t("modelsPanel.cancel")}
          </Button>
          <Button disabled={saving || !form.id.trim()} onClick={onSave} type="button">
            {saving ? t("modelsPanel.saving") : t("modelsPanel.save")}
          </Button>
        </div>
      </Panel>
    </div>
  );
}
