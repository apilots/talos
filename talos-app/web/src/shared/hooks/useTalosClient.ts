import { useMemo } from "react";

import { getTalosClient } from "@/shared/api/client";
import { useAppStore } from "@/shared/store/app-store";

export function useTalosClient() {
  const connection = useAppStore((state) => state.connection);
  return useMemo(() => getTalosClient(), [connection.authToken]);
}
