import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";

import type { ChatSlice, ChatSliceSetter } from "@/features/chat/store";
import { createChatSlice } from "@/features/chat/store";
import {
  createConnectionSlice,
  type ConnectionSlice,
} from "@/features/connection/store";
import {
  createUiSlice,
  normalizeProjects,
  SIDEBAR_WIDTH,
  type UiSlice,
} from "@/shared/store/ui-slice";
import { createPersistStorage } from "@/shared/store/persist-storage";

import { createComputerSlice, type ComputerSlice } from "@/features/computers/store";

import { connectionScope } from "./connection-scope";

export interface AppState
  extends ChatSlice,
    ConnectionSlice,
    UiSlice,
    ComputerSlice {}

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      ...createComputerSlice(set),
      ...createConnectionSlice(set as unknown as (updater: (state: ConnectionSlice) => Partial<ConnectionSlice>) => void),
      ...createUiSlice(set as unknown as (updater: (state: UiSlice) => Partial<UiSlice>) => void),
      ...createChatSlice(set as unknown as ChatSliceSetter),
    }),
    {
      name: "talos-web-ui",
      storage: createJSONStorage(createPersistStorage),
      merge: (persistedState, currentState) => {
        const typedState = persistedState as Partial<AppState> | undefined;
        return {
          ...currentState,
          selectedComputers: typedState?.selectedComputers ?? currentState.selectedComputers,
          composerDrafts: typedState?.composerDrafts ?? currentState.composerDrafts,
          sessionAliasesByScope: typedState?.sessionAliasesByScope ?? currentState.sessionAliasesByScope,
          sidebarOpen: typedState?.sidebarOpen ?? currentState.sidebarOpen,
          uiTheme: typedState?.uiTheme ?? currentState.uiTheme,
          locale: typedState?.locale ?? currentState.locale,
          activeProjectId: currentState.activeProjectId,
          transientStatuses: {},
          streams: {},
          sessionAliases: {},
          connection: currentState.connection,
        };
      },
      partialize: (state) => ({
        sessionAliasesByScope: { ...state.sessionAliasesByScope, [connectionScope(state.connection)]: state.sessionAliases },
        selectedComputers: state.selectedComputers,
        composerDrafts: state.composerDrafts,
        uiTheme: state.uiTheme,
        locale: state.locale,
        sidebarOpen: state.sidebarOpen,

      }),
    },
  ),
);

export function getConnectionSettings() {
  return useAppStore.getState().connection;
}

// Reset only actor-owned views. Computer switches preserve running Session observations.
useAppStore.subscribe((state, previous) => {
  const scope = connectionScope(state.connection);
  const oldScope = connectionScope(previous.connection);
  if (scope !== oldScope) {
    const aliases = { ...state.sessionAliasesByScope, [oldScope]: previous.sessionAliases };
    useAppStore.setState({ sessionAliasesByScope: aliases, sessionAliases: aliases[scope] ?? {}, streams: {}, transientStatuses: {}, workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false, sidebarWidth: SIDEBAR_WIDTH.initial, workspaceWidth: null, resizingPane: null, activeProjectId: normalizeProjects()[0].id });
  } else if (state.selectedComputers[scope] !== previous.selectedComputers[scope]) {
    useAppStore.setState({ activeProjectId: normalizeProjects()[0].id });
  }
});
