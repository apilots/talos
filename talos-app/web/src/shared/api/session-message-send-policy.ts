import type { AppProtocol } from "@talos/sdk-browser";
import { getTalosClient } from "./client";

function policyFromResponse(response: AppProtocol.ConfigResponse) {
  if (response.type !== "sessionMessageSendPolicy") {
    throw new Error("Expected session message send policy.");
  }
  return response.policy;
}

export function getSessionMessageSendPolicy(nodeId?: string) {
  return getTalosClient(nodeId).config.get("sessionMessageSendPolicy").then(policyFromResponse);
}

export function updateSessionMessageSendPolicy(nodeId: string, policy: AppProtocol.SessionMessageSendPolicy) {
  return getTalosClient(nodeId).config.update({ type: "sessionMessageSendPolicy", value: policy }).then(policyFromResponse);
}
