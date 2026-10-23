import type { LlmProtocol, ProviderModelConfig } from "@talos/sdk-browser";

export const CUSTOM_PROTOCOLS: LlmProtocol[] = ["chat", "messages", "responses"];
export const PROTOCOL_LABEL_KEYS = { chat: "modelsPanel.apiFormatChat", messages: "modelsPanel.protocolMessages", responses: "modelsPanel.protocolResponses" };

export function incompatibleReasoningReplayModel(models: ProviderModelConfig[], protocol: LlmProtocol) {
  return protocol === "chat" ? undefined : models.find((model) => (model.reasoning_replay ?? "protocol_default") !== "protocol_default");
}
