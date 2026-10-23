import type {
  ModelInfo,
  ModelRef,
  ModelSelection,
  ReasoningEffort,
} from "@talos/sdk-browser";

import type { ReasoningLevel } from "@/shared/types/ui";

export type SupportedAttachmentKind = "image" | "file";

export interface ComposerModelOption {
  id: string;
  name: string;
  provider: string;
  providerName?: string;
  customProvider?: boolean;
  reasoningEffort?: ReasoningEffort;
  selectionCapabilities?: ModelInfo["selection_capabilities"];
}

export function supportedAttachmentKinds(
  model: ModelInfo | undefined,
): SupportedAttachmentKind[] {
  return (model?.input ?? []).filter(
    (kind): kind is SupportedAttachmentKind => kind === "image" || kind === "file",
  );
}

export function reasoningLevelToEffort(level: ReasoningLevel): ReasoningEffort | null {
  if (level === "none") {
    return null;
  }
  return level === "extra_high" ? "xhigh" : level;
}

export function reasoningLevelForEffort(effort: ReasoningEffort | null): ReasoningLevel {
  if (effort === null) {
    return "none";
  }
  return effort === "xhigh" ? "extra_high" : effort;
}

export function modelRefKey(modelRef: ModelRef): string {
  return `${modelRef.provider}:${modelRef.id}`;
}

export function modelRefForKey(key: string | undefined): ModelRef | undefined {
  if (!key) {
    return undefined;
  }
  const separator = key.indexOf(":");
  if (separator <= 0 || separator === key.length - 1) {
    return undefined;
  }
  return { provider: key.slice(0, separator), id: key.slice(separator + 1) };
}

export function modelInfoForRef(
  models: ModelInfo[] | undefined,
  modelRef: ModelRef | undefined,
): ModelInfo | undefined {
  if (!modelRef) {
    return undefined;
  }
  return models?.find(
    (entry) => entry.provider === modelRef.provider && entry.id === modelRef.id,
  );
}

export function modelPickerOptions(models: ModelInfo[] | undefined): ComposerModelOption[] {
  return (models ?? []).map((model) => ({
    id: modelRefKey({ provider: model.provider, id: model.id }),
    name: model.name,
    provider: model.provider,
    providerName: model.provider_name ?? model.provider,
    customProvider: model.provider_name != null,
    reasoningEffort: model.reasoning_effort,
    selectionCapabilities: model.selection_capabilities,
  }));
}

export function modelSelectionForRef(
  models: ModelInfo[] | undefined,
  modelRef: ModelRef | undefined,
  reasoningLevel?: ReasoningLevel,
): ModelSelection | undefined {
  const model = modelInfoForRef(models, modelRef);
  if (!model) {
    return undefined;
  }
  const capabilities = model.selection_capabilities;
  const reasoningEffort = capabilities.reasoning !== "none"
    ? reasoningLevel === undefined
      ? capabilities.default_reasoning_effort
      : reasoningLevelToEffort(reasoningLevel)
    : null;
  return {
    modelRef: { provider: model.provider, id: model.id },
    reasoningEffort,
  };
}

export function reasoningLevelForModel(
  models: ModelInfo[] | undefined,
  modelRef: ModelRef | undefined,
): ReasoningLevel {
  const model = modelInfoForRef(models, modelRef);
  return reasoningLevelForEffort(model?.selection_capabilities.default_reasoning_effort ?? null);
}
