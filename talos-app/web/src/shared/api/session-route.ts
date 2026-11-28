/** Session links retain the product target across refresh and browser history. */
export function sessionPath(sessionId: string, computerId?: string | null): string {
  const path = `/sessions/${encodeURIComponent(sessionId)}`;
  return computerId ? `${path}?${new URLSearchParams({ computerId })}` : path;
}

/** Both the browser router and Desktop hash router expose the same route shape. */
export function sessionRouteComputer(
  sessionId: string,
  location: Pick<Location, "pathname" | "search" | "hash"> | undefined = typeof window === "undefined" ? undefined : window.location,
): string | undefined {
  if (!location) return undefined;
  const route = location.hash.startsWith("/") || location.hash.startsWith("#/")
    ? new URL(location.hash.replace(/^#/, ""), "http://router.local")
    : location;
  if (route.pathname !== `/sessions/${encodeURIComponent(sessionId)}`) return undefined;
  return new URLSearchParams(route.search).get("computerId")?.trim() || undefined;
}
