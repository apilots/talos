import { useQuery } from "@tanstack/react-query";

import { useEnvironmentKey } from "@/features/computers";
import { getLlmConfig } from "@/features/settings/api";

export function llmConfigKey(connectionKey: string) {
  return ["talos", connectionKey, "llm-config"] as const;
}

export function useLlmConfigQuery() {
  const connectionKey = useEnvironmentKey();

  return useQuery({
    queryKey: llmConfigKey(connectionKey),
    queryFn: getLlmConfig,
  });
}
