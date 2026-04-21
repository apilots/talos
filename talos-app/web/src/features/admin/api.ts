import type { LlmConfig } from "@talos/sdk";
import { getTalosClient } from "@/shared/api/client";

export async function getAdminLlmConfig(): Promise<LlmConfig> {
  return getTalosClient().admin.llm.get();
}

export async function updateAdminLlmConfig(config: LlmConfig): Promise<LlmConfig> {
  return getTalosClient().admin.llm.update(config);
}
