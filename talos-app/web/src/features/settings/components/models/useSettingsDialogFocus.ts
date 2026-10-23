import { useState, type RefObject } from "react";

/** Business triggers can disappear after a draft deletion; Radix owns the focus scope. */
export function useSettingsDialogFocus(fallback?: RefObject<HTMLElement | null>) {
  const [trigger] = useState(() => document.activeElement instanceof HTMLElement ? document.activeElement : null);
  return (event: Event) => {
    event.preventDefault();
    // The close callback runs after unmount. Preserve focus already moved elsewhere.
    if (document.activeElement !== document.body) return;
    const target = trigger?.isConnected ? trigger : fallback?.current;
    target?.focus();
  };
}
