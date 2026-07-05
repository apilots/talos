import { useEffect, useMemo, useState } from "react";

import { useMemoryPage } from "@/features/memory";
import { Panel } from "@/shared/ui/Panel";
import type { EditorMode } from "../AgentWorkspaceEditorPrimitives";
import { MemoryDailyDetail, MemorySearchDetail } from "./AgentWorkspaceMemoryDetailPanel";
import { AgentWorkspaceMemoryListPanel } from "./AgentWorkspaceMemoryListPanel";

interface AgentWorkspaceMemoryPanelProps {
  canEdit: boolean;
}

export function AgentWorkspaceMemoryPanel({ canEdit }: AgentWorkspaceMemoryPanelProps) {
  const memory = useMemoryPage();
  const [editorMode, setEditorMode] = useState<EditorMode>("preview");
  const showingSearch = memory.query.trim().length > 0;
  const memoryDates = useMemo(
    () => (showingSearch ? memory.searchResultDates : memory.files.map((item) => item.date)),
    [memory.files, memory.searchResultDates, showingSearch],
  );

  useEffect(() => {
    if (!canEdit) {
      setEditorMode("preview");
    }
  }, [canEdit]);

  return (
    <div className="grid gap-5 xl:grid-cols-[300px_minmax(0,1fr)]">
      <AgentWorkspaceMemoryListPanel dates={memoryDates} memory={memory} showingSearch={showingSearch} />
      <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/90 p-5">
        {showingSearch ? (
          <MemorySearchDetail memory={memory} />
        ) : (
          <MemoryDailyDetail
            canEdit={canEdit}
            editorMode={editorMode}
            memory={memory}
            setEditorMode={setEditorMode}
          />
        )}
      </Panel>
    </div>
  );
}
