import { describe, expect, it } from "vitest";
import type { ModelInfo } from "@talos/sdk-browser";

import {
  supportedAttachmentKinds,
  modelRefForKey,
  modelRefKey,
  modelSelectionForRef,
  reasoningLevelForModel,
  modelPickerOptions,
} from "./model-selection";

const models = [
  {
    provider: "openai",
    id: "shared-model",
    reasoning: true,
    reasoning_effort: "high",
    selection_capabilities: { reasoning: "effort", reasoning_efforts: ["light", "medium", "high", "xhigh"], fast: false, default_fast: false, default_reasoning_effort: "high" },
  },
  {
    provider: "ollama",
    id: "shared-model",
    reasoning: false,
    reasoning_effort: "medium",
    selection_capabilities: { reasoning: "none", reasoning_efforts: [], fast: false, default_fast: false, default_reasoning_effort: null },
  },
] as ModelInfo[];

describe("model selection", () => {
  it("round trips a provider-qualified model key", () => {
    const modelRef = { provider: "ollama", id: "qwen3:8b" };

    expect(modelRefForKey(modelRefKey(modelRef))).toEqual(modelRef);
  });

  it("selects the configured model when ids overlap across providers", () => {
    expect(
      modelSelectionForRef(models, { provider: "openai", id: "shared-model" }),
    ).toEqual({
      modelRef: { provider: "openai", id: "shared-model" },
      reasoningEffort: "high",
    });
    expect(
      modelSelectionForRef(models, { provider: "ollama", id: "shared-model" }),
    ).toEqual({
      modelRef: { provider: "ollama", id: "shared-model" },
      reasoningEffort: null,
    });
  });

  it("only exposes none for a model without configured reasoning", () => {
    expect(
      reasoningLevelForModel(models, { provider: "ollama", id: "shared-model" }),
    ).toBe("none");
  });

  it("returns only attachment inputs declared by the model", () => {
    expect(
      supportedAttachmentKinds({ input: ["text", "image", "audio", "file"] } as ModelInfo),
    ).toEqual(["image", "file"]);
    expect(supportedAttachmentKinds(undefined)).toEqual([]);
  });

  it("uses catalog reasoning defaults without inspecting request KV or model names", () => {
    const configured = [{ ...models[0], name: "Fast Reasoner", request_parameters: { enable_thinking: true }, selection_capabilities: { reasoning: "toggle", reasoning_efforts: [], fast: true, default_fast: true, default_reasoning_effort: null } }] as ModelInfo[];
    const ref = { provider: "openai", id: "shared-model" };
    expect(modelSelectionForRef(configured, ref)).toEqual({ modelRef: ref, reasoningEffort: null });
    expect(reasoningLevelForModel(configured, ref)).toBe("none");
    expect(modelSelectionForRef(configured, ref, "high")).toEqual({ modelRef: ref, reasoningEffort: "high" });
    expect(modelPickerOptions(configured)[0].selectionCapabilities).toEqual(configured[0].selection_capabilities);
  });
});
