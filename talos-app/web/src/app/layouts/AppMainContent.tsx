import type { ReactNode } from "react";

/** Route content retains its own scrolling/composer within one bounded surface. */
export function AppMainContent({ children }: { children: ReactNode }) {
  return <main className="h-full min-w-0 flex-1 overflow-hidden rounded-lg border border-border/50 bg-background">{children}</main>;
}
