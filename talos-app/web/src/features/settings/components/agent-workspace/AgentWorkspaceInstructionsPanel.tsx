import { selectionClasses } from "@/shared/ui/controlStyles";
import { useEffect, useState } from "react";
import { Save } from "lucide-react";
import { useTranslation } from "react-i18next";

import { formatWorkspaceDocDate, useWorkspaceDocsPage } from "@/features/workspace-docs";
import { Button } from "@/shared/ui/Button";
import { Textarea } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import {
  EditorModeSwitch,
  MarkdownPreview,
  type EditorMode,
} from "../AgentWorkspaceEditorPrimitives";

interface AgentWorkspaceInstructionsPanelProps {
  canEdit: boolean;
}

export function AgentWorkspaceInstructionsPanel({
  canEdit,
}: AgentWorkspaceInstructionsPanelProps) {
  const { t } = useTranslation(["settings", "workspaceDocs", "common"]);
  const [editorMode, setEditorMode] = useState<EditorMode>("preview");
  const workspaceDocs = useWorkspaceDocsPage();

  useEffect(() => {
    if (!canEdit) {
      setEditorMode("preview");
    }
  }, [canEdit]);

  return (
    <div className="grid gap-5 xl:grid-cols-[260px_minmax(0,1fr)]">
      <Panel className="rounded-[var(--radius-panel)] border-border/70 bg-background/[0.28] p-2.5 shadow-none backdrop-blur-none">
        <div className="mb-2 px-2 pt-2 text-[11px] uppercase tracking-[0.22em] text-muted/80">
          {t("workspaceDocs:documents")}
        </div>
        <div className="space-y-1">
          {workspaceDocs.docKinds.map((kind) => (
            <button
              className={[
                "w-full rounded-[var(--radius-control)] border px-3.5 py-3 text-left transition",
                workspaceDocs.selectedDocKind === kind
                  ? `${selectionClasses.selected} ${selectionClasses.border}`
                  : `border-transparent ${selectionClasses.idle}`,
              ].join(" ")}
              key={kind}
              onClick={() => workspaceDocs.setSelectedDocKind(kind)}
              type="button"
            >
              <div className="font-medium">{workspaceDocs.docLabels[kind]}</div>
              <div className="mt-1 text-xs text-muted">{kind}</div>
            </button>
          ))}
        </div>
      </Panel>
      <Panel className="rounded-[var(--radius-panel)] border-border/70 bg-background/35 p-5 shadow-none backdrop-blur-none">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="text-xs uppercase tracking-[0.22em] text-muted">{t("settings:agentInstructions")}</div>
            <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
              {workspaceDocs.docLabels[workspaceDocs.selectedDocKind]}
            </h2>
            <p className="mt-2 text-sm text-muted">
              {workspaceDocs.selectedDetail.exists
                ? t("workspaceDocs:lastUpdated", {
                    date: formatWorkspaceDocDate(workspaceDocs.selectedDetail.updated_at),
                  })
                : t("workspaceDocs:missingDetail")}
            </p>
          </div>
          {canEdit ? (
            <div className="flex items-center gap-3">
              <EditorModeSwitch mode={editorMode} setMode={setEditorMode} />
              {editorMode === "edit" ? (
                <Button
                  disabled={!workspaceDocs.isDirty || workspaceDocs.isSaving || workspaceDocs.isLoading}
                  onClick={() => void workspaceDocs.saveCurrent()}
                  type="button"
                >
                  <Save size={16} />
                  {workspaceDocs.isSaving ? t("workspaceDocs:saving") : t("common:save")}
                </Button>
              ) : null}
            </div>
          ) : null}
        </div>
        <div className="mt-4 rounded-[var(--radius-control)] border border-border/70 bg-background/[0.52] px-3 py-2 text-xs text-muted">
          {workspaceDocs.selectedDetail.path}
        </div>
        {workspaceDocs.loadError ? (
          <div className="mt-4 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
            {workspaceDocs.loadError}
          </div>
        ) : null}
        {editorMode === "edit" && canEdit ? (
          <Textarea
            className="mt-4 min-h-[440px] rounded-[var(--radius-panel)] border-border/70 bg-surface/[0.72] font-mono text-sm leading-6 shadow-none"
            onChange={(event) => workspaceDocs.setDocDraft(event.target.value)}
            value={workspaceDocs.docDraft}
          />
        ) : (
          <MarkdownPreview emptyText={t("workspaceDocs:emptyPreview")} text={workspaceDocs.selectedDetail.content} />
        )}
        {workspaceDocs.saveError ? (
          <div className="mt-3 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
            {workspaceDocs.saveError}
          </div>
        ) : null}
      </Panel>
    </div>
  );
}
