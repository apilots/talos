import type { CreateUserTokenResult, LlmConfig, UserTokenMeta } from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";

export type { CreateUserTokenResult, UserTokenMeta };

export async function getLlmConfig(): Promise<LlmConfig> {
  return getTalosClient().llm.get();
}

export async function updateLlmConfig(config: LlmConfig): Promise<LlmConfig> {
  return getTalosClient().llm.update(config);
}

export async function listUserTokens(): Promise<UserTokenMeta[]> {
  return getTalosClient().user.tokens.list();
}

export async function createUserToken(name: string): Promise<CreateUserTokenResult> {
  return getTalosClient().user.tokens.create({ name });
}

export async function revokeUserToken(tokenId: string): Promise<void> {
  return getTalosClient().user.tokens.delete(tokenId);
}
