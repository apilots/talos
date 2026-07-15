import { useEffect, useState } from "react";

export interface ComposerModelOption {
  id: string;
  name: string;
  provider?: string;
}

interface ComposerModelSelectProps {
  models: ComposerModelOption[];
  onChange?: (model: string) => void;
  selectedModel?: string;
  modelLabel: string;
  providerLabel: string;
}

export function useComposerModelOptions(
  models: ComposerModelOption[],
  selectedModel: string | undefined,
  onChange: ((model: string) => void) | undefined,
) {
  const providers = [...new Set(models.map((model) => model.provider ?? ""))];
  const selectedProvider = models.find((model) => model.id === selectedModel)?.provider ?? providers[0] ?? "";
  const [activeProvider, setActiveProvider] = useState(selectedProvider);

  useEffect(() => {
    if (selectedProvider) {
      setActiveProvider(selectedProvider);
    }
  }, [selectedProvider]);

  const providerModels = models.filter((model) => (model.provider ?? "") === activeProvider);
  const activeModel = providerModels.some((model) => model.id === selectedModel)
    ? selectedModel
    : providerModels[0]?.id;

  const changeProvider = (provider: string) => {
    setActiveProvider(provider);
    const firstConfiguredModel = models.find((model) => (model.provider ?? "") === provider);
    if (firstConfiguredModel) {
      onChange?.(firstConfiguredModel.id);
    }
  };

  return { activeModel, activeProvider, changeProvider, providerModels, providers };
}

export function ComposerModelSelect({
  models,
  onChange,
  selectedModel,
  modelLabel,
  providerLabel,
}: ComposerModelSelectProps) {
  const { activeModel, activeProvider, changeProvider, providerModels, providers } =
    useComposerModelOptions(models, selectedModel, onChange);

  return (
    <>
      <select
        aria-label={providerLabel}
        className="min-w-[8rem] flex-1 rounded-2xl border border-border bg-surface/80 px-3 py-2 text-sm outline-none transition focus:border-brand/50 md:max-w-[12rem]"
        disabled={providers.length <= 1}
        onChange={(event) => changeProvider(event.target.value)}
        value={activeProvider}
      >
        {providers.map((provider) => (
          <option key={provider || "models"} value={provider}>
            {provider || modelLabel}
          </option>
        ))}
      </select>
      <select
        aria-label={modelLabel}
        className="min-w-[10rem] flex-1 rounded-2xl border border-border bg-surface/80 px-3 py-2 text-sm outline-none transition focus:border-brand/50 md:max-w-[15rem]"
        onChange={(event) => onChange?.(event.target.value)}
        value={activeModel}
      >
        {providerModels.map((model) => (
          <option key={model.id} value={model.id}>
            {model.name}
          </option>
        ))}
      </select>
    </>
  );
}
