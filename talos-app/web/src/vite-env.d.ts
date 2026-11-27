/// <reference types="vite/client" />

import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

declare global {
  interface ImportMetaEnv {
    readonly VITE_API_BASE_URL?: string;
    readonly VITE_TALOS_CLOUD_AUTH_URL?: string;
    readonly VITE_TALOS_DESKTOP?: string;
  }

  const __TALOS_DEFAULT_PROJECT_ROOT__: string;

  interface Window {
    talos?: DesktopRendererBridge;
  }
}

export {};
