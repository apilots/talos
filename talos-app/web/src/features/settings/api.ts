import type { CreateUserTokenResult, LlmConfig, LlmConfigRead, UserTokenMeta } from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";

export type { CreateUserTokenResult, UserTokenMeta };

export async function getLlmConfig(): Promise<LlmConfigRead> {
  return getTalosClient().llm.get();
}

export async function updateLlmConfig(config: LlmConfig): Promise<LlmConfigRead> {
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
