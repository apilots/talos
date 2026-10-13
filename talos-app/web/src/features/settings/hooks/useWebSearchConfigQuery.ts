import { useQuery } from "@tanstack/react-query";

import { useEnvironmentKey } from "@/features/computers";
import { getWebSearchConfig } from "@/features/settings/api";

export function webSearchConfigKey(connectionKey: string) {
  return ["talos", connectionKey, "web-search-config"] as const;
}

export function useWebSearchConfigQuery() {
  const connectionKey = useEnvironmentKey();

  return useQuery({
    queryKey: webSearchConfigKey(connectionKey),
    queryFn: getWebSearchConfig,
  });
}
