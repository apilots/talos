import { SIDEBAR_WIDTH, WORKSPACE_WIDTH } from "@/shared/store/ui-slice";

export const MAIN_MIN_WIDTH = 320;
const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), max);

/** Clamp presentation preferences against the current viewport without changing them. */
export function layoutWidths(available: number, sidebarOpen: boolean, workspaceVisible: boolean, sidebarPreference: number, workspacePreference: number | null) {
  const sidebarMax = Math.max(SIDEBAR_WIDTH.min, Math.min(SIDEBAR_WIDTH.max, available - MAIN_MIN_WIDTH - (workspaceVisible ? WORKSPACE_WIDTH.min : 0)));
  const sidebar = clamp(sidebarPreference, SIDEBAR_WIDTH.min, sidebarMax);
  const workspaceMax = Math.max(WORKSPACE_WIDTH.min, Math.min(WORKSPACE_WIDTH.max, available - MAIN_MIN_WIDTH - (sidebarOpen ? sidebar : 0)));
  const workspace = workspaceVisible ? clamp(workspacePreference ?? WORKSPACE_WIDTH.initial, WORKSPACE_WIDTH.min, workspaceMax) : 0;
  return {
    sidebar, workspace,
    sidebarMax: Math.max(SIDEBAR_WIDTH.min, Math.min(SIDEBAR_WIDTH.max, available - workspace - MAIN_MIN_WIDTH)),
    workspaceMax,
  };
}
