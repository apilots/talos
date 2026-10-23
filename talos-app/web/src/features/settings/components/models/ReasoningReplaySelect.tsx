import type { ReasoningReplay } from "@talos/sdk-browser";
import { useId } from "react";
import { Select } from "@/shared/ui/Select";
import { useTranslation } from "react-i18next";

const POLICIES: ReasoningReplay[] = ["protocol_default", "omit", "reasoning", "reasoning_content"];

export function ReasoningReplaySelect({ value, onChange }: { value: ReasoningReplay; onChange: (policy: ReasoningReplay) => void }) {
  const { t } = useTranslation("settings");
  const id = useId();
  return (
    <div className="mt-4">
      <label className="mb-1.5 block text-sm font-medium text-ink" htmlFor={id}>{t("modelsPanel.reasoningReplay")}</label>
      <Select aria-describedby={`${id}-hint`} id={id} onValueChange={(next) => onChange(next as ReasoningReplay)} options={POLICIES.map((policy) => ({ value: policy, label: t(`modelsPanel.reasoningReplay_${policy}`) }))} value={value} />
      <p className="mt-1.5 text-xs text-muted" id={`${id}-hint`}>{t("modelsPanel.reasoningReplayHint")}</p>
    </div>
  );
}
