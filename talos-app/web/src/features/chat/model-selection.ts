import type {
  ModelInfo,
  ModelRef,
  ModelSelection,
  ReasoningEffort,
  SessionInfo,
} from "@talos/sdk-browser";

import type { ReasoningLevel } from "@/shared/types/ui";

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

export function firstModelRef(models: ModelInfo[] | undefined): ModelRef | undefined {
  const model = models?.[0];
  return model ? { provider: model.provider, id: model.id } : undefined;
}

export function modelPickerOptions(models: ModelInfo[] | undefined) {
  return (models ?? []).map((model) => ({
    id: modelRefKey({ provider: model.provider, id: model.id }),
    name: model.name,
    provider: model.provider,
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
  const reasoningEffort = model.reasoning
    ? reasoningLevel === undefined
      ? model.reasoning_effort
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
  return model?.reasoning ? reasoningLevelForEffort(model.reasoning_effort) : "none";
}

export function reasoningLevelForSession(
  session: SessionInfo | undefined,
  models: ModelInfo[] | undefined,
  draftModel: ModelRef | undefined,
): ReasoningLevel {
  if (!session) {
    return reasoningLevelForModel(models, draftModel);
  }
  return reasoningLevelForEffort(session.modelSelection.reasoningEffort);
}
