export interface ComposerRequest {
  sessionId: string;
  requestId: string;
  body: string;
}

export interface ComputerSlice {
  sessionAliasesByScope: Record<string, Record<string, string>>;
  selectedComputers: Record<string, string>;
  composerDrafts: Record<string, string>;
  composerRequests: Record<string, ComposerRequest>;
  selectComputer: (scope: string, nodeId: string) => void;
  clearComputer: (scope: string, nodeId: string) => void;
  saveComposerDraft: (key: string, draft: string) => void;
  saveComposerRequest: (key: string, request?: ComposerRequest) => void;
}
export function createComputerSlice(set: (update: (state: ComputerSlice) => Partial<ComputerSlice>) => void): ComputerSlice {
  return {
    sessionAliasesByScope: {}, selectedComputers: {}, composerDrafts: {}, composerRequests: {},
    selectComputer: (scope, nodeId) => set((state) => ({ selectedComputers: { ...state.selectedComputers, [scope]: nodeId } })),
    clearComputer: (scope, nodeId) => set((state) => state.selectedComputers[scope] === nodeId
      ? { selectedComputers: { ...state.selectedComputers, [scope]: "" } } : {}),
    saveComposerDraft: (key, draft) => set((state) => {
      const composerDrafts = { ...state.composerDrafts };
      const composerRequests = { ...state.composerRequests };
      if (state.composerDrafts[key] !== draft) delete composerRequests[key];
      if (draft) composerDrafts[key] = draft;
      else delete composerDrafts[key];
      return { composerDrafts, composerRequests };
    }),
    saveComposerRequest: (key, request) => set((state) => {
      const composerRequests = { ...state.composerRequests };
      if (request) composerRequests[key] = request;
      else delete composerRequests[key];
      return { composerRequests };
    }),
  };
}
