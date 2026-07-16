import { useQuery } from "@tanstack/react-query";

import { useConnectionKey } from "@/features/chat";
import { getProfile } from "../api";

export function profileKey(connectionKey: string) {
  return ["profile", connectionKey] as const;
}

export function useProfileQuery() {
  const connectionKey = useConnectionKey();

  return useQuery({
    queryKey: profileKey(connectionKey),
    queryFn: getProfile,
  });
}
