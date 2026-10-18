export interface ComputerSlice {
  sessionAliasesByScope: Record<string, Record<string, string>>;
  selectedComputers: Record<string, string>;
  composerDrafts: Record<string, string>;
  selectComputer: (scope: string, nodeId: string) => void;
  clearComputer: (scope: string, nodeId: string) => void;
  saveComposerDraft: (key: string, draft: string) => void;
}
export function createComputerSlice(set: (update: (state: ComputerSlice) => Partial<ComputerSlice>) => void): ComputerSlice {
  return {
    sessionAliasesByScope: {}, selectedComputers: {}, composerDrafts: {},
    selectComputer: (scope, nodeId) => set((state) => ({ selectedComputers: { ...state.selectedComputers, [scope]: nodeId } })),
    clearComputer: (scope, nodeId) => set((state) => state.selectedComputers[scope] === nodeId
      ? { selectedComputers: { ...state.selectedComputers, [scope]: "" } } : {}),
    saveComposerDraft: (key, draft) => set((state) => {
      const composerDrafts = { ...state.composerDrafts };
      if (draft) composerDrafts[key] = draft;
      else delete composerDrafts[key];
      return { composerDrafts };
    }),
  };
}
