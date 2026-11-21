import { useQuery } from "@tanstack/react-query";

import { fileMetadataKey } from "../api";
import { getTalosClient } from "@/shared/api/client";

/** Shares the existing account/endpoint-scoped Files metadata cache. */
export function useFileMetadata(connectionKey: string, fileId: string, owner: { sessionId?: string; nodeId?: string }) {
  return useQuery({
    queryKey: fileMetadataKey(connectionKey, fileId, owner),
    queryFn: async () => (await getTalosClient({ service: "files", ...owner })).files.read({ fileId }),
    select: (response) => response.file,
    refetchOnWindowFocus: false,
    retry: false,
  });
}
