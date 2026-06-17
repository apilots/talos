import type {
  ModelInputKind,
  ModelSpeed,
  ProviderModelConfig,
  ReasoningEffort,
} from "@talos/sdk-browser";
import type React from "react";
import { File, Image, Mic, Type, Video } from "lucide-react";

export const INPUT_KINDS: ModelInputKind[] = ["text", "image", "audio", "video", "file"];
export const REASONING_EFFORTS: ReasoningEffort[] = ["low", "medium", "high", "xhigh"];
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
}

export const emptyModelForm: ModelFormState = {
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
    id: form.id,
    name: form.name,
    context_window: 0,
    reasoning: false,
    reasoning_effort: "medium",
    speed: "standard",
    input: ["text"],
    cost: { input: 0, output: 0, cache_read: 0, cache_write: 0 },
    compat: {},
  };

  return {
    ...base,
    id: form.id.trim(),
    name: form.name.trim(),
    context_window: parseInt(form.context_window, 10) || 0,
    reasoning: form.reasoning,
    reasoning_effort: form.reasoning_effort,
    speed: form.speed,
    input: form.input.length > 0 ? form.input : ["text"],
  };
}
