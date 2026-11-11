import { queryOptions } from "@tanstack/react-query";
import type { AppProtocol } from "@talos/sdk-browser";
import { getTalosClient } from "./client";

function policyFromResponse(response: AppProtocol.ConfigResponse) {
  if (response.type !== "sessionMessageSendPolicy") {
    throw new Error("Expected session message send policy.");
  }
  return response.policy;
}

export function sessionMessageSendPolicyOptions(scope: string, nodeId?: string) {
  return queryOptions({
    queryKey: ["talos", scope, "session-message-send-policy", nodeId],
    queryFn: () => getTalosClient(nodeId).config.get("sessionMessageSendPolicy").then(policyFromResponse),
    enabled: Boolean(nodeId),
    refetchOnWindowFocus: true,
  });
}

export function updateSessionMessageSendPolicy(nodeId: string, policy: AppProtocol.SessionMessageSendPolicy) {
  return getTalosClient(nodeId).config.update({ type: "sessionMessageSendPolicy", value: policy }).then(policyFromResponse);
}
