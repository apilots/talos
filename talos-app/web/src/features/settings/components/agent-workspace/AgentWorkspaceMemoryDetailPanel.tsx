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
        {memory.isLoadingMemory ? (
          <div className="rounded-[var(--radius-control)] border border-border/65 bg-background/38 px-3.5 py-4 text-sm text-muted">
            {t("common:loading")}
          </div>
        ) : null}
        {memory.memoryError ? (
          <div className="rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3.5 py-3 text-sm text-danger">
            {memory.memoryError}
          </div>
        ) : null}
        {memory.selectedMemory ? (
          <div className="rounded-[var(--radius-control)] border border-brand/25 bg-background/70 px-4 py-4">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div>
                <div className="text-xs uppercase tracking-[0.18em] text-muted">
                  {memory.selectedMemory.kind} · {memory.selectedMemory.importance}
                </div>
                <div className="mt-2 text-base font-medium text-ink">{memory.selectedMemory.title}</div>
              </div>
              <Button onClick={() => memory.selectDate(memory.selectedMemory!.source.date)}>
                {t("memory:openDailyMemory")}
              </Button>
            </div>
            <div className="mt-3 whitespace-pre-wrap text-sm leading-6 text-ink">{memory.selectedMemory.body}</div>
          </div>
        ) : null}
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
              key={result.memoryId}
              onClick={() => memory.selectMemory(result.memoryId)}
              type="button"
            >
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div className="text-xs uppercase tracking-[0.18em] text-muted">
                  {result.source.date}
                </div>
                <div className="text-sm font-medium text-ink">{result.title}</div>
              </div>
              <div className="mt-3 text-sm leading-6 text-ink">{result.preview}</div>
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
          className="mt-4 min-h-[440px] rounded-[var(--radius-panel)] border-border/70 bg-surface/72 font-mono text-sm leading-6 shadow-none"
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
