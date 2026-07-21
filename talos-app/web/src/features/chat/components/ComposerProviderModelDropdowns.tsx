import type React from "react";
import { Waypoints, Zap } from "lucide-react";

import { ComposerDropdown } from "./ComposerDropdown";
import {
  type ComposerModelOption,
  useComposerModelOptions,
} from "./ComposerModelSelect";

interface ComposerProviderModelDropdownsProps {
  modelLabel: string;
  models: ComposerModelOption[];
  onChange?: (model: string) => void;
  providerLabel: string;
  selectedModel?: string;
  modelTriggerRef?: React.Ref<HTMLButtonElement>;
}

export function ComposerProviderModelDropdowns({
  modelLabel,
  models,
  onChange,
  providerLabel,
  selectedModel,
  modelTriggerRef,
}: ComposerProviderModelDropdownsProps) {
  const { activeModel, activeProvider, changeProvider, providerModels, providers } =
    useComposerModelOptions(models, selectedModel, onChange);

  return (
    <>
      <ComposerDropdown
        disabled={providers.length <= 1}
        header={providerLabel}
        items={providers.map((provider) => ({
          value: provider,
          label: provider || modelLabel,
        }))}
        onChange={changeProvider}
        triggerIcon={<Waypoints size={12} className="text-brand" />}
        value={activeProvider}
      />
      <ComposerDropdown
        header={modelLabel}
        items={providerModels.map((model) => ({ value: model.id, label: model.name }))}
        onChange={(model) => onChange?.(model)}
        triggerRef={modelTriggerRef}
        triggerIcon={<Zap size={12} className="text-brand" />}
        value={activeModel}
      />
    </>
  );
}
