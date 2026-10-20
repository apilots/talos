import { useQuery } from "@tanstack/react-query";

import { getTalosClient } from "@/shared/api/client";

/** Shares the existing account/endpoint-scoped Files metadata cache. */
export function useFileMetadata(connectionKey: string, fileId: string) {
  return useQuery({
    queryKey: ["talos", connectionKey, "file", fileId],
    queryFn: () => getTalosClient().files.read({ fileId }),
    select: (response) => response.file,
    refetchOnWindowFocus: false,
    retry: false,
  });
}
