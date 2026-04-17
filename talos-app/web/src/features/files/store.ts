import { createDefaultFiles } from "@/shared/types/domain";
import type { FileRecord } from "@/shared/types/ui";

export interface FilesSlice {
  fileItems: FileRecord[];
  upsertFile: (record: FileRecord) => void;
  removeFile: (id: string) => void;
}

export type FilesSliceSetter = (updater: (state: FilesSlice) => Partial<FilesSlice>) => void;

export function createFilesSlice(set: FilesSliceSetter): FilesSlice {
  return {
    fileItems: createDefaultFiles(),
    upsertFile: (record) =>
      set((state) => ({
        fileItems: [record, ...state.fileItems.filter((item) => item.id !== record.id)],
      })),
    removeFile: (id) =>
      set((state) => ({
        fileItems: state.fileItems.filter((item) => item.id !== id),
      })),
  };
}
