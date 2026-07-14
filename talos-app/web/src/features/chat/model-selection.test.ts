import { describe, expect, it } from "vitest";
import type { ModelInfo } from "@talos/sdk-browser";

import {
  modelRefForKey,
  modelRefKey,
  modelSelectionForRef,
  reasoningLevelForModel,
} from "./model-selection";

const models = [
  {
    provider: "openai",
    id: "shared-model",
    reasoning: true,
    reasoning_effort: "high",
  },
  {
    provider: "ollama",
    id: "shared-model",
    reasoning: false,
    reasoning_effort: "medium",
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
});
