import { createBrowserTalosClient } from "@talos/sdk";

import { getConnectionSettings } from "@/shared/store/app-store";

export function getTalosClient() {
  const { serverUrl, authToken } = getConnectionSettings();
  return createBrowserTalosClient({
    baseUrl: serverUrl,
    getToken: () => authToken || undefined,
  });
}
