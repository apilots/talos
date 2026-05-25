import type { LlmConfig, LlmConfigRead } from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";

export async function getLlmConfig(): Promise<LlmConfigRead> {
  return getTalosClient().llm.get();
}

export async function updateLlmConfig(config: LlmConfig): Promise<LlmConfigRead> {
  return getTalosClient().llm.update(config);
}
