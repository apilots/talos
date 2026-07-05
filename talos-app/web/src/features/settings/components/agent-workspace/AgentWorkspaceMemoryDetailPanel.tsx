import { Save } from "lucide-react";
import { useTranslation } from "react-i18next";

import { formatMemoryDate, useMemoryPage } from "@/features/memory";
import { Button } from "@/shared/ui/Button";
import { Textarea } from "@/shared/ui/Input";
import {
  EditorModeSwitch,
  MarkdownPreview,
  type EditorMode,
} from "../AgentWorkspaceEditorPrimitives";

type MemoryPageState = ReturnType<typeof useMemoryPage>;

export function MemorySearchDetail({ memory }: { memory: MemoryPageState }) {
  const { t } = useTranslation(["common", "memory"]);
  const busy = memory.isSearchPending || memory.isSearching;

  return (
    <div>
      <div className="text-xs uppercase tracking-[0.22em] text-muted">{t("common:search")}</div>
      <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
        {busy ? t("memory:searching") : t("memory:matchingSnippets")}
      </h2>
      <p className="mt-2 text-sm text-muted">
        {busy ? t("memory:detail") : t("memory:searchResultsCount", { count: memory.searchResults.length })}
      </p>
      <div className="mt-5 space-y-3">
        {memory.searchError ? (
          <div className="rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3.5 py-3 text-sm text-danger">
            {memory.searchError}
          </div>
        ) : null}
        {memory.searchResults.length === 0 && !busy ? (
          <div className="rounded-[var(--radius-control)] border border-dashed border-border/65 bg-background/38 px-3.5 py-4 text-sm text-muted">
            {t("memory:nothingMatched")}
          </div>
        ) : (
          memory.searchResults.map((result) => (
            <button
              className="w-full rounded-[var(--radius-control)] border border-border/65 bg-background/50 px-4 py-4 text-left transition hover:border-brand/30 hover:bg-background/74"
              key={`${result.date}:${result.start_line}:${result.end_line}`}
              onClick={() => memory.selectDate(result.date)}
              type="button"
            >
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="text-xs uppercase tracking-[0.18em] text-muted">{result.date}</div>
                <div className="rounded-[var(--radius-control)] border border-border/65 bg-background/68 px-2.5 py-1 text-[11px] text-muted">
                  {t("memory:lineRange", { start: result.start_line, end: result.end_line })}
                </div>
              </div>
              <div className="mt-3 text-sm leading-6 text-ink">{result.snippet}</div>
            </button>
          ))
        )}
      </div>
    </div>
  );
}

export function MemoryDailyDetail({
  canEdit,
  editorMode,
  memory,
  setEditorMode,
}: {
  canEdit: boolean;
  editorMode: EditorMode;
  memory: MemoryPageState;
  setEditorMode: (mode: EditorMode) => void;
}) {
  const { t } = useTranslation(["common", "memory"]);

  return (
    <div>
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <div className="text-xs uppercase tracking-[0.22em] text-muted">{memory.selectedDate}</div>
          <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
            {memory.selectedDetail.version_token ? t("memory:editorTitle") : t("memory:newFileTitle")}
          </h2>
          <p className="mt-2 text-sm text-muted">
            {memory.selectedDetail.version_token
              ? t("memory:lastUpdated", { date: formatMemoryDate(memory.selectedDetail.updated_at) })
              : t("memory:newFileDetail")}
          </p>
        </div>
        {canEdit ? <MemorySaveControls editorMode={editorMode} memory={memory} setEditorMode={setEditorMode} /> : null}
      </div>
      {editorMode === "edit" && canEdit ? (
        <Textarea
          className="mt-4 min-h-[440px] rounded-[var(--radius-panel)] border-border/70 bg-[linear-gradient(180deg,rgba(255,255,255,0.32),rgba(255,255,255,0.16))] font-mono text-sm leading-6 shadow-[inset_0_1px_0_rgba(255,255,255,0.5)]"
          onChange={(event) => memory.setEditorContent(event.target.value)}
          value={memory.editorContent}
        />
      ) : (
        <MarkdownPreview emptyText={t("memory:emptyPreview")} text={memory.selectedDetail.content} />
      )}
      {memory.saveError ? (
        <div className="mt-3 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
          {memory.saveError}
        </div>
      ) : null}
    </div>
  );
}

function MemorySaveControls({
  editorMode,
  memory,
  setEditorMode,
}: {
  editorMode: EditorMode;
  memory: MemoryPageState;
  setEditorMode: (mode: EditorMode) => void;
}) {
  const { t } = useTranslation(["common", "memory"]);

  return (
    <div className="flex items-center gap-3">
      <EditorModeSwitch mode={editorMode} setMode={setEditorMode} />
      {editorMode === "edit" ? (
        <Button disabled={!memory.isDirty || memory.isSaving || memory.isLoadingDetail} onClick={() => void memory.saveCurrent()} type="button">
          <Save size={16} />
          {memory.isSaving ? t("memory:saving") : t("common:save")}
        </Button>
      ) : null}
    </div>
  );
}
