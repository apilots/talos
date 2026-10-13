import { defaultProductEndpoint } from "@/shared/store/connection-scope";

/** Web addresses its selected management service. Desktop uses the stdio host. */
export function resolveServerBaseUrl(): string { return defaultProductEndpoint(); }
