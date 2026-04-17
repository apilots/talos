import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";

import {
  createAdminSlice,
  type AdminSlice,
} from "@/features/admin/store";
import type { ChatSlice, ChatSliceSetter } from "@/features/chat/store";
import { createChatSlice } from "@/features/chat/store";
import {
  createConnectionSlice,
  type ConnectionSlice,
} from "@/features/connection/store";
import {
  createFilesSlice,
  type FilesSlice,
} from "@/features/files/store";
import {
  createMemorySlice,
  type MemorySlice,
} from "@/features/memory/store";
import {
  createUiSlice,
  mergeById,
  normalizeProjects,
  type UiSlice,
} from "@/shared/store/ui-slice";
import { createPersistStorage } from "@/shared/store/persist-storage";

export interface AppState
  extends ChatSlice,
    ConnectionSlice,
    UiSlice,
    MemorySlice,
    FilesSlice,
    AdminSlice {}

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      ...createConnectionSlice(set as unknown as (updater: (state: ConnectionSlice) => Partial<ConnectionSlice>) => void),
      ...createUiSlice(set as unknown as (updater: (state: UiSlice) => Partial<UiSlice>) => void),
      ...createMemorySlice(set as unknown as (updater: (state: MemorySlice) => Partial<MemorySlice>) => void),
      ...createFilesSlice(set as unknown as (updater: (state: FilesSlice) => Partial<FilesSlice>) => void),
      ...createAdminSlice(set as unknown as (updater: (state: AdminSlice) => Partial<AdminSlice>) => void),
      ...createChatSlice(set as unknown as ChatSliceSetter),
    }),
    {
      name: "talos-web-ui",
      storage: createJSONStorage(createPersistStorage),
      merge: (persistedState, currentState) => {
        const typedState = persistedState as Partial<AppState> | undefined;
        const projects = normalizeProjects(typedState?.projects);
        const activeProjectExists = projects.some(
          (project) => project.id === typedState?.activeProjectId,
        );
        return {
          ...currentState,
          ...typedState,
          uiTheme: typedState?.uiTheme ?? currentState.uiTheme,
          locale: typedState?.locale ?? currentState.locale,
          threadFilter: "all",
          projects,
          activeProjectId: activeProjectExists ? typedState?.activeProjectId ?? projects[0].id : projects[0].id,
          sessionApprovalModes: typedState?.sessionApprovalModes ?? {},
          transientStatuses: typedState?.transientStatuses ?? {},
          streams: typedState?.streams ?? {},
          memoryItems: mergeById(currentState.memoryItems, typedState?.memoryItems),
          fileItems: mergeById(currentState.fileItems, typedState?.fileItems),
          connection: {
            ...currentState.connection,
            ...typedState?.connection,
            defaultApprovalMode: typedState?.connection?.defaultApprovalMode ?? currentState.connection.defaultApprovalMode,
          },
        };
      },
      partialize: (state) => ({
        connection: state.connection,
        uiTheme: state.uiTheme,
        locale: state.locale,
        sidebarOpen: state.sidebarOpen,
        activeProjectId: state.activeProjectId,
        projects: state.projects,
        sessionAliases: state.sessionAliases,
        sessionApprovalModes: state.sessionApprovalModes,
        transientStatuses: state.transientStatuses,
        streams: state.streams,
        memoryItems: state.memoryItems,
        fileItems: state.fileItems,
      }),
    },
  ),
);

export function getConnectionSettings() {
  return useAppStore.getState().connection;
}
