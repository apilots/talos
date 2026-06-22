import { createDefaultMemory } from "@/shared/types/domain";
import type { MemoryItemView } from "@/shared/types/ui";

export interface MemorySlice {
  memoryItems: MemoryItemView[];
  upsertMemory: (record: MemoryItemView) => void;
  removeMemory: (id: string) => void;
}

export type MemorySliceSetter = (updater: (state: MemorySlice) => Partial<MemorySlice>) => void;

export function createMemorySlice(set: MemorySliceSetter): MemorySlice {
  return {
    memoryItems: createDefaultMemory(),
    upsertMemory: (record) =>
      set((state) => ({
        memoryItems: [record, ...state.memoryItems.filter((item) => item.id !== record.id)],
      })),
    removeMemory: (id) =>
      set((state) => ({
        memoryItems: state.memoryItems.filter((item) => item.id !== id),
      })),
  };
}
