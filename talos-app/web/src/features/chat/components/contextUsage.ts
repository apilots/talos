import type { SessionInfo } from "@talos/sdk-browser";

export type ContextUsage = NonNullable<SessionInfo["usage"]["context"]>;

export function contextUsedPercent(usage: ContextUsage): number {
  if (usage.contextWindowTokens <= 0) {
    return 0;
  }
  return Math.min(100, Math.max(0, (usage.usedTokens / usage.contextWindowTokens) * 100));
}

export function contextRemainingTokens(usage: ContextUsage): number {
  return Math.max(0, usage.contextWindowTokens - usage.usedTokens);
}
