import { getConnectionSettings } from "@/shared/store/app-store";
import { defaultProductEndpoint } from "@/shared/store/connection-scope";

export function resolveServerBaseUrl(): string {
  const connection = getConnectionSettings();
  return connection.desktopConnectionMode === "cloud" && connection.controlPlaneEndpoint
    ? connection.controlPlaneEndpoint : defaultProductEndpoint();
}
