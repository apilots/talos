import { useQuery } from "@tanstack/react-query";

import { useEnvironmentKey, useProductScope } from "@/features/computers";
import { getLlmConfig } from "@/features/settings/api";

export function llmConfigKey(connectionKey: string) {
  return ["talos", connectionKey, "llm-config"] as const;
}

export function useLlmEnvironmentKey(nodeId?: string) {
  const scope = useProductScope();
  const defaultEnvironmentKey = useEnvironmentKey();
  return nodeId ? JSON.stringify([scope, nodeId]) : defaultEnvironmentKey;
}

export function useLlmConfigQuery(nodeId?: string) {
  const connectionKey = useLlmEnvironmentKey(nodeId);

  return useQuery({
    queryKey: llmConfigKey(connectionKey),
    queryFn: () => (nodeId ? getLlmConfig(nodeId) : getLlmConfig()),
  });
}
