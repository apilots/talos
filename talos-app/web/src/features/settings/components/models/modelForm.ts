import { parseRequestParameters, type RequestParameterRow } from "./requestParameters";

import type {
  ModelInputKind,
  ModelSpeed,
  ProviderModelConfig,
  ReasoningEffort,
  ReasoningReplay,
} from "@talos/sdk-browser";
import type React from "react";
import { File, Image, Mic, Type, Video } from "lucide-react";

export const INPUT_KINDS: ModelInputKind[] = ["text", "image", "audio", "video", "file"];
export const REASONING_EFFORTS: ReasoningEffort[] = ["light", "medium", "high", "xhigh"];
export const MODEL_SPEEDS: ModelSpeed[] = ["standard", "fast"];

export const INPUT_KIND_ICONS: Record<ModelInputKind, React.ElementType> = {
  text: Type,
  image: Image,
  audio: Mic,
  video: Video,
  file: File,
};

export interface ModelFormState {
  id: string;
  name: string;
  context_window: string;
  reasoning: boolean;
  reasoning_effort: ReasoningEffort;
  speed: ModelSpeed;
  input: ModelInputKind[];
  request_parameters: RequestParameterRow[];
  reasoning_replay: ReasoningReplay;
}

export const emptyModelForm: ModelFormState = {
  request_parameters: [],
  reasoning_replay: "protocol_default",
  id: "",
  name: "",
  context_window: "200000",
  reasoning: false,
  reasoning_effort: "medium",
  speed: "standard",
  input: ["text"],
};

export function modelToForm(model: ProviderModelConfig): ModelFormState {
  return {
    reasoning_replay: model.reasoning_replay ?? "protocol_default",
    request_parameters: Object.entries(model.request_parameters ?? {}).map(([key, value]) => ({ key, value: JSON.stringify(value) })),
    id: model.id,
    name: model.name,
    context_window: String(model.context_window),
    reasoning: model.reasoning,
    reasoning_effort: model.reasoning_effort ?? "medium",
    speed: model.speed ?? "standard",
    input: model.input,
  };
}

export function formToModel(
  form: ModelFormState,
  existing?: ProviderModelConfig,
): ProviderModelConfig {
  const base: ProviderModelConfig = existing ?? {
    request_parameters: {},
    id: form.id,
    name: form.name,
    context_window: 0,
    reasoning: false,
    reasoning_effort: "medium",
    speed: "standard",
    input: ["text"],
    cost: { input: 0, output: 0, cache_read: 0, cache_write: 0 },
  };

  return {
    ...base,
    request_parameters: parseRequestParameters(form.request_parameters).value,
    reasoning_replay: form.reasoning_replay,
    id: form.id.trim(),
    name: form.name.trim(),
    context_window: parseInt(form.context_window, 10) || 0,
    reasoning: form.reasoning,
    reasoning_effort: form.reasoning_effort,
    speed: form.speed,
    input: form.input.length > 0 ? form.input : ["text"],
  };
}
