import type { AppProtocol } from "@talos/sdk-core";
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
  cloud_auth_url?: string | null;
}

const envCloudAuthUrl = import.meta.env.VITE_TALOS_CLOUD_AUTH_URL as string | undefined;

export async function logoutUser(): Promise<void> {
  if (isDesktopRuntime()) {
    await clearDesktopCloudSession();
  }
  return getTalosClient().auth.logout();
}

export async function getCurrentUser(): Promise<UserDto> {
  const response = await getTalosClient().user.current();
  return response.user;
}

export async function getCurrentUserWithToken(accessToken: string): Promise<UserDto> {
  if (isDesktopRuntime()) {
    const bridge = getDesktopBridge();
    if (!bridge) {
      throw new Error("Desktop runtime bridge is not available.");
    }
    const response = await bridge.invoke<AppProtocol.UserCurrentResponse>(
      "user.current",
      {},
      {
        actorCredentials: {
          credential: {
            type: "existing_session_token",
            token: accessToken,
          },
        },
      },
    );
    return response.user;
  }

  const { serverUrl } = getConnectionSettings();
  const response = await fetch(new URL("/v1/user", serverUrl).toString(), {
    headers: {
      Accept: "application/json",
      Authorization: `Bearer ${accessToken}`,
    },
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  const payload = await response.json() as AppProtocol.UserCurrentResponse;
  return payload.user;
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
      cloud_auth_url: serverInfo.cloud_auth_url,
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

export async function getCloudAuthUrl(): Promise<string> {
  const health = await getGatewayHealth();
  const configured = health.cloud_auth_url?.trim() || envCloudAuthUrl?.trim();
  if (!configured) {
    throw new Error("Cloud auth URL is not configured.");
  }
  return configured;
}

export interface CloudLoginStartResponse {
  login_id: string;
  login_url: string;
  user_code: string;
  expires_at_ms: number;
  interval_ms: number;
}

export type CloudLoginStatusResponse =
  | { status: "pending"; interval_ms: number }
  | { status: "authenticated"; user: UserDto }
  | { status: "denied" }
  | { status: "expired" };

export async function startCloudLogin(): Promise<CloudLoginStartResponse> {
  if (isDesktopRuntime()) {
    const bridge = getDesktopBridge();
    if (!bridge) {
      throw new Error("Desktop runtime bridge is not available.");
    }
    return bridge.invoke<CloudLoginStartResponse>("auth.login.start", {
      client_name: "Talos Desktop",
      client_kind: "desktop",
    });
  }

  const { serverUrl } = getConnectionSettings();
  const response = await fetch(new URL("/v1/auth/login/start", serverUrl).toString(), {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      client_name: "Talos Web",
      client_kind: "web",
    }),
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<CloudLoginStartResponse>;
}

export async function getCloudLoginStatus(loginId: string): Promise<CloudLoginStatusResponse> {
  if (isDesktopRuntime()) {
    const bridge = getDesktopBridge();
    if (!bridge) {
      throw new Error("Desktop runtime bridge is not available.");
    }
    return bridge.invoke<CloudLoginStatusResponse>("auth.login.status", {
      login_id: loginId,
    });
  }

  const { serverUrl } = getConnectionSettings();
  const url = new URL("/v1/auth/login/status", serverUrl);
  url.searchParams.set("login_id", loginId);
  const response = await fetch(url.toString(), {
    headers: {
      Accept: "application/json",
    },
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<CloudLoginStatusResponse>;
}

export async function readDesktopCloudSession(): Promise<AppProtocol.AuthSessionSnapshot | null> {
  if (!isDesktopRuntime()) {
    return null;
  }
  const bridge = getDesktopBridge();
  if (!bridge) {
    return null;
  }
  const response = await bridge.invoke<AppProtocol.AuthSessionReadResponse>(
    "auth.session.read",
    {},
  );
  return response.session ?? null;
}

export async function storeDesktopCloudSession(
  accessToken: string,
  cloudApiBaseUrl: string,
  user: UserDto,
): Promise<void> {
  const session = buildCloudSessionSnapshot(accessToken, cloudApiBaseUrl, user);
  if (!isDesktopRuntime()) {
    const { serverUrl } = getConnectionSettings();
    const response = await fetch(new URL("/v1/auth/session", serverUrl).toString(), {
      method: "POST",
      headers: {
        Accept: "application/json",
        Authorization: `Bearer ${accessToken}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ session }),
    });
    if (!response.ok) {
      throw new Error(`${response.status} ${response.statusText}`);
    }
    return;
  }
  const bridge = getDesktopBridge();
  if (!bridge) {
    return;
  }
  await bridge.invoke(
    "auth.session.store",
    { session },
    {
      actorCredentials: {
        credential: {
          type: "existing_session_token",
          token: accessToken,
        },
      },
    },
  );
}

function buildCloudSessionSnapshot(
  accessToken: string,
  cloudApiBaseUrl: string,
  user: UserDto,
): AppProtocol.AuthSessionSnapshot {
  return {
    schema_version: 1,
    cloud_api_base_url: cloudApiBaseUrl,
    access_token: accessToken,
    token_type: "Bearer",
    expires_at_ms: null,
    last_used_at_ms: Date.now().toString(),
    user: {
      user_id: user.user_id,
      username: user.username,
      email: user.email ?? null,
      role: user.role,
    },
  };
}

export async function clearDesktopCloudSession(): Promise<void> {
  if (!isDesktopRuntime()) {
    return;
  }
  const bridge = getDesktopBridge();
  if (!bridge) {
    return;
  }
  await bridge.invoke("auth.session.clear", {});
}
