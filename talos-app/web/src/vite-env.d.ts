/// <reference types="vite/client" />

import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

declare global {
  const __TALOS_DEFAULT_PROJECT_ROOT__: string;

  interface Window {
    talos?: DesktopRendererBridge;
  }
}

export {};
