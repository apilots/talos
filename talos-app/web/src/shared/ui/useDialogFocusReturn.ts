import { useState, type RefObject } from "react";

/** Restore only after a user closes a dialog; never compete with another surface. */
export function useDialogFocusReturn(fallback?: RefObject<HTMLElement | null>) {
  const [trigger] = useState(() => document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null);
  return (event: Event) => {
    event.preventDefault();
    if (document.activeElement !== document.body) return;
    const target = trigger?.isConnected ? trigger : fallback?.current;
    if (target?.isConnected) target.focus();
  };
}
