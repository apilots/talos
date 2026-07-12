import type {
  ModelInfo,
  ModelSelection,
  ReasoningEffort,
  SessionInfo,
} from "@talos/sdk-browser";

import type { ReasoningLevel } from "@/shared/types/ui";

export function reasoningLevelToEffort(level: ReasoningLevel): ReasoningEffort {
  return level === "extra_high" ? "xhigh" : level;
}

export function reasoningLevelForEffort(effort: ReasoningEffort): ReasoningLevel {
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
  const selectedEffort = reasoningLevel && model.reasoning
    ? reasoningLevelToEffort(reasoningLevel)
    : undefined;
  const reasoning = selectedEffort && selectedEffort !== model.reasoning_effort
    ? { type: "effort" as const, level: selectedEffort }
    : { type: "modelDefault" as const };
  return {
    modelRef: { provider: model.provider, id: model.id },
    reasoning,
  };
}

export function reasoningLevelForModel(
  models: ModelInfo[] | undefined,
  modelId: string | undefined,
): ReasoningLevel {
  const model = models?.find((entry) => entry.id === modelId);
  return reasoningLevelForEffort(model?.reasoning_effort ?? "medium");
}

export function reasoningLevelForSession(
  session: SessionInfo | undefined,
  models: ModelInfo[] | undefined,
  draftModel: string | undefined,
): ReasoningLevel {
  if (!session) {
    return reasoningLevelForModel(models, draftModel);
  }
  if (session.modelSelection.reasoning.type === "effort") {
    return reasoningLevelForEffort(session.modelSelection.reasoning.level);
  }
  return reasoningLevelForModel(models, session.modelSelection.modelRef.id);
}
