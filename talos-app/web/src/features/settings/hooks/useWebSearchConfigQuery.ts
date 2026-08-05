import { useQuery } from "@tanstack/react-query";

import { useConnectionKey } from "@/features/chat";
import { getWebSearchConfig } from "@/features/settings/api";

export function webSearchConfigKey(connectionKey: string) {
  return ["web-search-config", connectionKey] as const;
}

export function useWebSearchConfigQuery() {
  const connectionKey = useConnectionKey();

  return useQuery({
    queryKey: webSearchConfigKey(connectionKey),
    queryFn: getWebSearchConfig,
  });
}
