import type { UserDto } from "@talos/sdk-browser";
import { getDesktopServerInfo } from "@talos/sdk-app-server/desktop";

import { getTalosClient } from "@/shared/api/client";
import { getDesktopBridge, isDesktopRuntime } from "@/shared/api/desktop-client";
import { getConnectionSettings } from "@/shared/store/app-store";

export interface HealthResponse {
  status: string;
  service: string;
  timestamp: string;
  auth_enabled: boolean;
}

export async function logoutUser(): Promise<void> {
  return getTalosClient().auth.logout();
}

export async function getCurrentUser(): Promise<UserDto> {
  return getTalosClient().user.current();
}

export async function getGatewayHealth(): Promise<HealthResponse> {
  if (isDesktopRuntime()) {
    const bridge = getDesktopBridge();
    if (!bridge) {
      throw new Error("Desktop runtime bridge is not available.");
    }
    const serverInfo = await getDesktopServerInfo(bridge);
    return {
      status: "ok",
      service: serverInfo.name,
      timestamp: new Date().toISOString(),
      auth_enabled: serverInfo.auth_enabled,
    };
  }

  const { serverUrl } = getConnectionSettings();
  const response = await fetch(new URL("/health", serverUrl).toString(), {
    headers: {
      Accept: "application/json",
    },
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<HealthResponse>;
}
