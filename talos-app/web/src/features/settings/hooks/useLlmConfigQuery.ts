import { useQuery } from "@tanstack/react-query";

import { useConnectionKey } from "@/features/chat";
import { getLlmConfig } from "@/features/settings/api";

export function llmConfigKey(connectionKey: string) {
  return ["llm-config", connectionKey] as const;
}

export function useLlmConfigQuery() {
  const connectionKey = useConnectionKey();

  return useQuery({
    queryKey: llmConfigKey(connectionKey),
    queryFn: getLlmConfig,
  });
}
