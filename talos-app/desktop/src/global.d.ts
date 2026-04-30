import type { TalosDesktopBridge } from "./ipc";

declare global {
  interface Window {
    talos?: TalosDesktopBridge;
  }
}

export {};
