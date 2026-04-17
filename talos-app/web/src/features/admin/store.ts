import type { AdminMetric } from "@/shared/types/ui";

export interface AdminSlice {
  adminMetrics: AdminMetric[];
  setAdminMetrics: (metrics: AdminMetric[]) => void;
}

export type AdminSliceSetter = (updater: (state: AdminSlice) => Partial<AdminSlice>) => void;

export function createAdminSlice(set: AdminSliceSetter): AdminSlice {
  return {
    adminMetrics: [],
    setAdminMetrics: (adminMetrics) => set(() => ({ adminMetrics })),
  };
}
