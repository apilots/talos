import { ChevronRight, RotateCcw, Zap } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";

import type { ComposerModelOption } from "../../model-selection";
import { reasoningLevelForEffort } from "../../model-selection";
import type { ReasoningLevel } from "@/shared/types/ui";
import { ModelPickerEffortSlider } from "./ModelPickerEffortSlider";
import styles from "./ModelPicker.module.css";

const EFFORT_LEVELS: ReasoningLevel[] = ["none", "light", "medium", "high", "extra_high"];

export function ModelPickerOverview({ model, modelName, providerName, effort, fast, onFastChange, onEffortChange, onModels }: {
  model?: ComposerModelOption;
  modelName: string;
  providerName: string;
  effort: ReasoningLevel;
  fast: boolean;
  onFastChange: (enabled: boolean) => void;
  onEffortChange?: (level: ReasoningLevel) => void;
  onModels: () => void;
}) {
  const { t } = useTranslation("chat");
  const tooltipId = useId();
  const capabilities = model?.selectionCapabilities;
  const reasoningControl = capabilities?.reasoning ?? "none";
  const supportsFast = Boolean(capabilities?.fast);
  const onLevel = reasoningLevelForEffort(model?.reasoningEffort ?? null);
  const levels = reasoningControl === "effort" ? EFFORT_LEVELS : reasoningControl === "toggle" ? ["none" as const, onLevel] : ["none" as const];
  const effectiveEffort = reasoningControl === "none" ? "none" : reasoningControl === "toggle" && effort !== "none" ? onLevel : effort;
  const [preview, setPreview] = useState(effectiveEffort);
  useEffect(() => setPreview(effectiveEffort), [effectiveEffort]);
  const defaultEffort = reasoningLevelForEffort(capabilities?.default_reasoning_effort ?? null);
  const label = (level: ReasoningLevel) => level === "none" ? t("modelPicker.off") : reasoningControl === "toggle" ? t("modelPicker.on") : t(level === "extra_high" ? "reasoningExtraHigh" : level === "light" ? "reasoningLight" : level === "medium" ? "reasoningMedium" : "reasoningHigh");
  const reasoningDisabled = reasoningControl === "none" || !onEffortChange;

  return (
    <>
      <div className={styles.overviewHead}>
        <div className={styles.fastSlot}>
          <button aria-describedby={tooltipId} aria-disabled={!supportsFast} aria-label={t("modelPicker.fast")} aria-pressed={fast} className={styles.fastToggle} onClick={() => { if (supportsFast) onFastChange(!fast); }} type="button"><Zap aria-hidden fill="currentColor" size={15} strokeWidth={1} /></button>
          <span className={styles.controlTooltip} id={tooltipId} role="tooltip">{t(fast ? "modelPicker.fastOn" : "modelPicker.fastOff")}<span className={styles.tooltipDetail}>{t(supportsFast ? "modelPicker.fastIndependent" : "modelPicker.fastUnavailable")}</span></span>
        </div>
        <span className={styles.currentLevel} data-off={preview === "none"}>{label(preview)}</span>
        <button aria-label={t("modelPicker.reset", { depth: label(defaultEffort) })} className={styles.resetEffort} disabled={reasoningDisabled} onClick={() => { setPreview(defaultEffort); onEffortChange?.(defaultEffort); }} type="button"><RotateCcw aria-hidden size={14} strokeWidth={1.3} /></button>
      </div>
      <button aria-label={t("modelPicker.modelSelect", { model: modelName, provider: providerName })} className={styles.currentModel} onClick={onModels} title={`${providerName} · ${modelName}`} type="button"><span className={styles.modelName}>{modelName}</span><ChevronRight aria-hidden size={12} /></button>
      <ModelPickerEffortSlider disabled={reasoningDisabled} label={t(reasoningControl === "none" ? "modelPicker.reasoningUnavailable" : "modelPicker.depth")} levels={levels} onChange={onEffortChange} onPreviewChange={setPreview} value={preview} valueLabel={label} />
    </>
  );
}
