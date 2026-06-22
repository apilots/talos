import type { ConfigGetResponse, ConfigUpdateResponse, LlmConfig } from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";

export async function getLlmConfig(): Promise<ConfigGetResponse> {
  return getTalosClient().llm.get();
}

export async function updateLlmConfig(config: LlmConfig): Promise<ConfigUpdateResponse> {
  return getTalosClient().llm.update(config);
}
