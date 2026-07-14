interface ComposerModelSelectProps {
  models: Array<{ id: string; name: string; provider?: string }>;
  onChange?: (model: string) => void;
  selectedModel?: string;
  fallbackGroupLabel: string;
}

export function composerModelItems(models: ComposerModelSelectProps["models"]) {
  return models.map((model) => ({
    value: model.id,
    label: model.name,
    group: model.provider,
  }));
}

export function ComposerModelSelect({
  models,
  onChange,
  selectedModel,
  fallbackGroupLabel,
}: ComposerModelSelectProps) {
  const providers = [...new Set(models.map((model) => model.provider ?? ""))];

  return (
    <select
      className="min-w-[10rem] flex-1 rounded-2xl border border-border bg-surface/80 px-3 py-2 text-sm outline-none transition focus:border-brand/50 md:max-w-[15rem]"
      onChange={(event) => onChange?.(event.target.value)}
      value={selectedModel}
    >
      {providers.map((provider) => (
        <optgroup key={provider || "models"} label={provider || fallbackGroupLabel}>
          {models
            .filter((model) => (model.provider ?? "") === provider)
            .map((model) => (
              <option key={model.id} value={model.id}>
                {model.name}
              </option>
            ))}
        </optgroup>
      ))}
    </select>
  );
}
