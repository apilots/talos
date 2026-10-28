import { Archive, ArrowLeft, ChevronRight, FileText } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { FilePreviewEntry } from "@talos/sdk-core";
import { formatFileSize } from "../hooks/useFilesPage";

/** The workspace tab remains the archive; this breadcrumb identifies its current member. */
export function ArchiveNavigation({ name, path, onBack }: { name: string; path: string; onBack: () => void }) {
  const { t } = useTranslation("files");
  return <nav aria-label={t("preview.archiveNavigation")} className="flex shrink-0 flex-wrap items-center gap-x-2 gap-y-1 border-b border-divider bg-surface px-4 py-2 text-xs">
    <button type="button" aria-label={t("preview.backToArchive")} title={name} onClick={onBack} className="inline-flex min-w-0 max-w-full items-center gap-2 rounded py-1 text-brand hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand">
      <ArrowLeft aria-hidden="true" size={13} className="shrink-0" /><Archive aria-hidden="true" size={13} className="shrink-0" /><span className="truncate">{name}</span>
    </button>
    <ChevronRight aria-hidden="true" size={12} className="shrink-0 text-muted" />
    <span className="min-w-0 break-all text-ink" title={path}>{path}</span>
  </nav>;
}

export function ArchiveContents({ name, entries, onSelect }: { name: string; entries: FilePreviewEntry[]; onSelect: (entry: FilePreviewEntry) => void }) {
  const { t } = useTranslation("files");
  return <div className="min-h-0 flex-1 overflow-auto p-4">
    <header className="mb-4 flex min-w-0 items-start gap-3">
      <span className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-surface text-muted"><Archive aria-hidden="true" size={17} /></span>
      <div className="min-w-0"><h1 className="break-all text-sm font-medium">{name}</h1><p className="mt-0.5 text-xs text-muted">{t("preview.archiveEntries", { count: entries.length })}</p></div>
    </header>
    {entries.length ? <ul aria-label={t("preview.archiveContents")} className="divide-y divide-divider rounded-lg border border-border">
      {entries.map(entry => <li key={entry.resourcePath}><button type="button" onClick={() => onSelect(entry)} className="flex w-full items-center gap-3 px-3 py-3 text-left text-xs hover:bg-hover focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand">
        <FileText aria-hidden="true" size={15} className="shrink-0 text-muted" /><span className="min-w-0 flex-1 break-all">{entry.path}</span><span className="shrink-0 tabular-nums text-muted">{formatFileSize(entry.metadata.sizeBytes)}</span><ChevronRight aria-hidden="true" size={13} className="shrink-0 text-muted" />
      </button></li>)}
    </ul> : <p className="rounded-lg border border-border px-4 py-6 text-sm text-muted">{t("preview.archiveEmpty")}</p>}
  </div>;
}
