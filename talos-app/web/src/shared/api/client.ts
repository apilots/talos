import { createBrowserTalosClient } from "@talos/sdk-browser";

import { resolveServerBaseUrl } from "@/features/connection/store";
import { createDesktopTalosClient, getDesktopBridge } from "./desktop-client";
import { getConnectionSettings } from "@/shared/store/app-store";

export type TalosWebClient = ReturnType<typeof createBrowserTalosClient>;
export type TalosClientFactory = () => TalosWebClient;

let clientFactory: TalosClientFactory | undefined;

function createDefaultBrowserClient() {
  const desktopBridge = getDesktopBridge();
  if (desktopBridge) {
    return createDesktopTalosClient(desktopBridge, getAuthTokenForRequests);
  }

  return createBrowserTalosClient({
    baseUrl: resolveServerBaseUrl(),
    getToken: getAuthTokenForRequests,
  });
}

function getAuthTokenForRequests(): string | undefined {
  const connection = getConnectionSettings();
  if (connection.authStatus === "open") {
    return undefined;
  }
  return connection.authToken || undefined;
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
