import { createBrowserTalosClient } from "@talos/sdk-browser";

import { getConnectionSettings } from "@/shared/store/app-store";

export type TalosWebClient = ReturnType<typeof createBrowserTalosClient>;
export type TalosClientFactory = () => TalosWebClient;

let clientFactory: TalosClientFactory | undefined;

function createDefaultBrowserClient() {
  const { serverUrl, authToken } = getConnectionSettings();
  return createBrowserTalosClient({
    baseUrl: serverUrl,
    getToken: () => authToken || undefined,
  });
}

export function setTalosClientFactory(factory: TalosClientFactory) {
  clientFactory = factory;
}

export function resetTalosClientFactory() {
  clientFactory = undefined;
}

export function getTalosClient() {
  return (clientFactory ?? createDefaultBrowserClient)();
}
