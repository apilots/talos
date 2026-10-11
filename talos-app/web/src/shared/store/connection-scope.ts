import type { ConnectionState } from "@/features/connection/store";

export function defaultProductEndpoint() {
  return import.meta.env.VITE_API_BASE_URL?.trim()
    || (typeof window !== "undefined" && window.location.origin !== "null" ? window.location.origin : "http://localhost:8080");
}
export function normalizeEndpoint(endpoint: string) {
  return new URL(endpoint).href.replace(/\/$/, "");
}
/** Actor and control-plane identity, excluding rotating credentials. */
export function connectionScope(connection: ConnectionState) {
  const endpoint = connection.desktopConnectionMode === "local" ? "desktop-local"
    : normalizeEndpoint(connection.controlPlaneEndpoint || defaultProductEndpoint());
  return JSON.stringify([endpoint, connection.userId]);
}
