import type {
  ConfigGetResponse,
  ConfigUpdateResponse,
  LlmConfig,
  ProfileReadResponse,
} from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";

export async function getLlmConfig(): Promise<ConfigGetResponse> {
  return getTalosClient().llm.get();
}

export async function updateLlmConfig(config: LlmConfig): Promise<ConfigUpdateResponse> {
  return getTalosClient().llm.update(config);
}

export async function getProfile(): Promise<ProfileReadResponse> {
  try {
    return await getTalosClient().profile.read();
  } catch (error) {
    console.error("Failed to read profile data.", error);
    throw error;
  }
}
