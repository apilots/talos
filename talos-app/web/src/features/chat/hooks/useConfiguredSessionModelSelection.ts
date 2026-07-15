import { useEffect, useRef } from "react";
import type { ModelListResponse, ModelSelection } from "@talos/sdk-browser";

import { modelInfoForRef, modelRefKey } from "../model-selection";

interface ConfiguredSessionModelSelectionOptions {
  catalog: ModelListResponse | undefined;
  selection: ModelSelection | undefined;
  sessionId: string;
  updateSelection: (selection: ModelSelection) => void;
}

/** Resolves a session selection against the configured model catalog and persists its default repair. */
export function useConfiguredSessionModelSelection({
  catalog,
  selection,
  sessionId,
  updateSelection,
}: ConfiguredSessionModelSelectionOptions): ModelSelection | undefined {
  const repairedSelectionKeyRef = useRef<string | null>(null);
  const selectionExists = selection
    ? Boolean(modelInfoForRef(catalog?.models, selection.modelRef))
    : false;
  const needsDefaultSelection = Boolean(selection && catalog && !selectionExists);
  const resolvedSelection = needsDefaultSelection ? catalog?.defaultModelSelection : selection;
  const repairKey = needsDefaultSelection && catalog && selection
    ? `${sessionId}:${modelRefKey(selection.modelRef)}:${modelRefKey(catalog.defaultModelSelection.modelRef)}:${catalog.defaultModelSelection.reasoningEffort ?? "none"}`
    : null;

  useEffect(() => {
    if (!repairKey || !catalog) {
      return;
    }
    if (repairedSelectionKeyRef.current === repairKey) {
      return;
    }
    repairedSelectionKeyRef.current = repairKey;
    updateSelection(catalog.defaultModelSelection);
  }, [catalog, repairKey, updateSelection]);

  return resolvedSelection;
}
