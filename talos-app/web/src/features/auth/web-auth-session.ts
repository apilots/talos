import { isDesktopRuntime } from "@/shared/api/desktop-client";

const WEB_AUTH_TOKEN_KEY = "talos-web-auth-token";

export function readWebAuthToken(): string | null {
  if (isDesktopRuntime() || typeof window === "undefined") {
    return null;
  }
  try {
    const token = window.sessionStorage.getItem(WEB_AUTH_TOKEN_KEY)?.trim();
    return token || null;
  } catch {
    return null;
  }
}

export function storeWebAuthToken(token: string): void {
  if (isDesktopRuntime() || typeof window === "undefined") {
    return;
  }
  const normalized = token.trim();
  if (!normalized) {
    clearWebAuthToken();
    return;
  }
  try {
    window.sessionStorage.setItem(WEB_AUTH_TOKEN_KEY, normalized);
  } catch {
    // The in-memory authenticated session remains valid when browser storage is unavailable.
  }
}

export function clearWebAuthToken(): void {
  if (isDesktopRuntime() || typeof window === "undefined") {
    return;
  }
  try {
    window.sessionStorage.removeItem(WEB_AUTH_TOKEN_KEY);
  } catch {
    // There is no persisted token to clear when browser storage is unavailable.
  }
}
