import { defaultProductEndpoint } from "@/shared/store/connection-scope";

/** Return the management endpoint selected by the Web client. */
export function resolveServerBaseUrl(): string { return defaultProductEndpoint(); }
