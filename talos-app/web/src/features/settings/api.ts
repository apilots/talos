import type {
  ConfigResponse,
  LlmConfig,
  ProfileReadResponse,
  WebSearchToolConfig,
} from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";

export type LlmConfigResponse = Extract<ConfigResponse, { type: "llm" }>;
export type WebSearchConfigResponse = Extract<ConfigResponse, { type: "webSearch" }>;

function expectLlmConfig(response: ConfigResponse): LlmConfigResponse {
  if (response.type !== "llm") {
    throw new Error("Expected llm config response.");
  }
  return response;
}

function expectWebSearchConfig(
  response: ConfigResponse,
): WebSearchConfigResponse {
  if (response.type !== "webSearch") {
    throw new Error("Expected web search config response.");
  }
  return response;
}

export async function getLlmConfig(): Promise<LlmConfigResponse> {
  return expectLlmConfig(await getTalosClient().llm.get());
}

export async function updateLlmConfig(config: LlmConfig): Promise<LlmConfigResponse> {
  return expectLlmConfig(await getTalosClient().llm.update(config));
}

export async function getWebSearchConfig(): Promise<WebSearchConfigResponse> {
  return expectWebSearchConfig(await getTalosClient().config.get("webSearch"));
}

export async function updateWebSearchConfig(
  config: WebSearchToolConfig,
): Promise<WebSearchConfigResponse> {
  return expectWebSearchConfig(
    await getTalosClient().config.update({ type: "webSearch", value: config }),
  );
}

export async function getProfile(): Promise<ProfileReadResponse> {
  try {
    return await getTalosClient().profile.read();
  } catch (error) {
    console.error("Failed to read profile data.", error);
    throw error;
  }
}
