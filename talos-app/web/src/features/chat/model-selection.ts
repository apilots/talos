import type {
  ModelInfo,
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

export function modelSelectionForId(
  models: ModelInfo[] | undefined,
  modelId: string | undefined,
  reasoningLevel?: ReasoningLevel,
): ModelSelection | undefined {
  const model = models?.find((entry) => entry.id === modelId);
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
  modelId: string | undefined,
): ReasoningLevel {
  const model = models?.find((entry) => entry.id === modelId);
  return model?.reasoning ? reasoningLevelForEffort(model.reasoning_effort) : "none";
}

export function reasoningLevelForSession(
  session: SessionInfo | undefined,
  models: ModelInfo[] | undefined,
  draftModel: string | undefined,
): ReasoningLevel {
  if (!session) {
    return reasoningLevelForModel(models, draftModel);
  }
  return reasoningLevelForEffort(session.modelSelection.reasoningEffort);
}
