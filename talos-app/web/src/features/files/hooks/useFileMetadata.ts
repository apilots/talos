import { useQuery } from "@tanstack/react-query";

import { getTalosClient } from "@/shared/api/client";

/** Shares the existing account/endpoint-scoped Files metadata cache. */
export function useFileMetadata(connectionKey: string, fileId: string, sessionId?: string) {
  return useQuery({
    queryKey: ["talos", connectionKey, "file", fileId, ...(sessionId ? [sessionId] : [])],
    queryFn: () => getTalosClient().files.read({ fileId, sessionId }),
    select: (response) => response.file,
    refetchOnWindowFocus: false,
    retry: false,
  });
}
