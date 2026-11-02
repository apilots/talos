import { cleanup } from "@testing-library/react";
import { vi } from "vitest";

const originalScroll = Element.prototype.scrollIntoView;

export function installTracingDom({ narrow = true } = {}) {
  Element.prototype.scrollIntoView = vi.fn();
  vi.stubGlobal("matchMedia", () => ({ matches: narrow }));
  vi.stubGlobal("ResizeObserver", class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
}

export function cleanupTracingDom() {
  cleanup();
  if (originalScroll) Element.prototype.scrollIntoView = originalScroll;
  else Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  vi.unstubAllGlobals();
}
