export interface ComputerSlice {
  sessionAliasesByScope: Record<string, Record<string, string>>;
  selectedComputers: Record<string, string>;
  composerDrafts: Record<string, string>;
  composerDraftSubmissions: Partial<Record<string, symbol>>;
  selectComputer: (scope: string, nodeId: string) => void;
  clearComputer: (scope: string, nodeId: string) => void;
  saveComposerDraft: (key: string, draft: string) => void;
  beginComposerDraftSubmission: (key: string, draft: string) => ((accepted: boolean) => void) | undefined;
}
export function createComputerSlice(set: (update: (state: ComputerSlice) => Partial<ComputerSlice>) => void): ComputerSlice {
  return {
    sessionAliasesByScope: {}, selectedComputers: {}, composerDrafts: {},
    composerDraftSubmissions: {},
    selectComputer: (scope, nodeId) => set((state) => ({ selectedComputers: { ...state.selectedComputers, [scope]: nodeId } })),
    clearComputer: (scope, nodeId) => set((state) => state.selectedComputers[scope] === nodeId
      ? { selectedComputers: { ...state.selectedComputers, [scope]: "" } } : {}),
    saveComposerDraft: (key, draft) => set((state) => {
      const composerDrafts = { ...state.composerDrafts };
      if (draft) composerDrafts[key] = draft;
      else delete composerDrafts[key];
      const composerDraftSubmissions = { ...state.composerDraftSubmissions };
      delete composerDraftSubmissions[key];
      return { composerDrafts, composerDraftSubmissions };
    }),
    beginComposerDraftSubmission: (key, draft) => {
      if (!draft) return;
      // Keep the saved draft recoverable across a refresh until admission succeeds.
      const submission = Symbol();
      set((state) => {
        if (state.composerDrafts[key] !== draft || state.composerDraftSubmissions[key]) return state;
        return { composerDraftSubmissions: { ...state.composerDraftSubmissions, [key]: submission } };
      });
      return (accepted) => set((state) => {
        if (state.composerDraftSubmissions[key] !== submission) return state;
        const composerDraftSubmissions = { ...state.composerDraftSubmissions };
        delete composerDraftSubmissions[key];
        if (!accepted) return { composerDraftSubmissions };
        const composerDrafts = { ...state.composerDrafts };
        delete composerDrafts[key];
        return { composerDraftSubmissions, composerDrafts };
      });
    },
  };
}
