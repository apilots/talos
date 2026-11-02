import * as Popover from "@radix-ui/react-popover";
import { ChevronDown, Zap } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type Ref } from "react";
import { useTranslation } from "react-i18next";

import type { ReasoningLevel } from "@/shared/types/ui";
import type { ComposerModelOption } from "../model-selection";
import { ModelPickerOverview } from "./model-picker/ModelPickerOverview";
import { ModelPickerModelList, ModelPickerProviderList, type ModelPickerProvider } from "./model-picker/ModelPickerLists";
import styles from "./model-picker/ModelPicker.module.css";

type View = "overview" | "models" | "providers";

interface ComposerModelPickerProps {
  disabled?: boolean;
  models: ComposerModelOption[];
  selectedModel?: string;
  reasoningLevel: ReasoningLevel;
  modelTriggerRef?: Ref<HTMLButtonElement>;
  onModelChange?: (model: string) => void;
  onReasoningLevelChange?: (level: ReasoningLevel) => void;
  onOpenChange?: (open: boolean) => void;
}

export function ComposerModelPicker({ disabled, models, selectedModel, reasoningLevel, modelTriggerRef, onModelChange, onReasoningLevelChange, onOpenChange }: ComposerModelPickerProps) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<View>("overview");
  const [browsingProvider, setBrowsingProvider] = useState<string>();
  const contentRef = useRef<HTMLDivElement>(null);
  const didNavigate = useRef(false);
  const activeModel = models.find((model) => model.id === selectedModel);
  const defaultFast = activeModel?.selectionCapabilities?.default_fast ?? false;
  const [fastState, setFastState] = useState({ model: selectedModel, enabled: defaultFast });
  const fastPreview = fastState.model === selectedModel ? fastState.enabled : defaultFast;
  useEffect(() => setFastState({ model: selectedModel, enabled: defaultFast }), [selectedModel, defaultFast]);
  const providers = [...new Map(models.map((model) => [model.provider, { id: model.provider, name: model.providerName ?? model.provider, custom: Boolean(model.customProvider) }])).values()];
  const providerName = (provider?: string) => providers.find((item) => item.id === provider)?.name ?? provider ?? t("providerLabel");
  const browsingProviderId = browsingProvider ?? activeModel?.provider ?? providers[0]?.id;
  const modelName = activeModel?.name ?? t("modelLabel");
  const control = activeModel?.selectionCapabilities?.reasoning ?? "none";
  const effortLabel = control === "none" || reasoningLevel === "none" ? t("modelPicker.off") : control === "toggle" ? t("modelPicker.on") : t(reasoningLevel === "extra_high" ? "reasoningExtraHigh" : reasoningLevel === "light" ? "reasoningLight" : reasoningLevel === "medium" ? "reasoningMedium" : "reasoningHigh");
  const triggerLabel = t(fastPreview ? "modelPicker.fastTrigger" : "modelPicker.trigger", { model: modelName, effort: effortLabel });

  function navigate(next: View) { didNavigate.current = true; setView(next); }
  useLayoutEffect(() => {
    if (!didNavigate.current) return;
    contentRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus({ preventScroll: true });
    didNavigate.current = false;
  }, [view]);
  useEffect(() => { onOpenChange?.(open); }, [onOpenChange, open]);
  useEffect(() => { if (disabled) setOpen(false); }, [disabled]);

  return (
    <Popover.Root onOpenChange={(next) => { setOpen(next); setView("overview"); }} open={open}>
      <Popover.Trigger asChild><button aria-label={triggerLabel} className={styles.trigger} disabled={disabled || models.length === 0} ref={modelTriggerRef} title={`${providerName(activeModel?.provider)} · ${modelName}${fastPreview ? ` · ${t("modelPicker.fastIndependent")}` : ""}`} type="button">
        {fastPreview && <Zap aria-hidden className={styles.triggerFast} fill="currentColor" size={12} strokeWidth={1} />}<span className={styles.triggerName}>{modelName}</span><span className={styles.triggerEffort}>· {effortLabel}</span><ChevronDown aria-hidden size={11} />
      </button></Popover.Trigger>
      <Popover.Portal><Popover.Content align="end" aria-label={t("modelPicker.dialog")} className={styles.picker} collisionPadding={12} data-view={view}
        onOpenAutoFocus={(event) => {
          const control = contentRef.current?.querySelector<HTMLElement>('[role="slider"]:not([aria-disabled="true"])')
            ?? contentRef.current?.querySelector<HTMLElement>('button:not(:disabled):not([aria-disabled="true"])');
          if (control) { event.preventDefault(); control.focus({ preventScroll: true }); }
        }}
        onEscapeKeyDown={(event) => { if (view !== "overview") { event.preventDefault(); navigate(view === "providers" ? "models" : "overview"); } }} ref={contentRef} side="top" sideOffset={10}>
        {view === "overview" && <ModelPickerOverview effort={reasoningLevel} fast={fastPreview} model={activeModel} modelName={modelName}
          onEffortChange={onReasoningLevelChange ? (level) => { if (level !== reasoningLevel) onReasoningLevelChange(level); } : undefined}
          onFastChange={(enabled) => setFastState({ model: selectedModel, enabled })} onModels={() => { setBrowsingProvider(activeModel?.provider); navigate("models"); }} providerName={providerName(activeModel?.provider)} />}
        {view === "models" && <ModelPickerModelList models={models.filter((model) => model.provider === browsingProviderId)} onBack={() => navigate("overview")}
          onProvider={() => navigate("providers")} onSelect={(model) => { if (model.id !== selectedModel) onModelChange?.(model.id); navigate("overview"); }} providerName={providerName(browsingProviderId)} selectedModel={selectedModel} />}
        {view === "providers" && <ModelPickerProviderList onBack={() => navigate("models")} onSelect={(provider: ModelPickerProvider) => { setBrowsingProvider(provider.id); navigate("models"); }} providers={providers} selectedProvider={activeModel?.provider} />}
      </Popover.Content></Popover.Portal>
    </Popover.Root>
  );
}
