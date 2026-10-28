import { lazy, Suspense, useEffect, useRef, useState, type ReactNode } from "react";
import { parseFilePreviewReference, type FilePreviewEntry, type FilePreviewSource } from "@talos/sdk-core";
import type { FilePreviewPosition } from "@/shared/store/workspace-window";
import { Download, FileText, RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useBrowserSurface } from "@/features/web-preview/useBrowserSurface";
import { BrowserFrame, BrowserToolbar } from "@/features/web-preview/BrowserToolbar";
import { MarkdownRenderer } from "@/widgets/message-list/MarkdownRenderer";
import { openResource, openResourceLink } from "@/shared/lib/open-resource";
import { fileDisplay } from "../file-preview";
import { useFilePreview, usePreviewContent, usePreviewMetadata } from "../hooks/useFilePreview";
import { downloadFile } from "../hooks/useFileContent";
import { formatFileSize } from "../hooks/useFilesPage";
import { ArchiveContents, ArchiveNavigation } from "./ArchivePreview";
import { TextPreview } from "./TextPreview";
const PdfPreview = lazy(() => import("./PdfPreview"));

function DocumentImage({ fileId, label, scope, instanceKey }: { fileId: string; label: string; scope: string; instanceKey: string }) {
  const { t } = useTranslation("files");
  const preview = useFilePreview({ kind: "file", fileId }, scope, `${instanceKey}:image:${fileId}`);
  return preview.error ? <span role="alert">{userVisibleErrorMessage(preview.error, t("preview.loadFailed"))}</span>
    : preview.descriptor ? <img src={getTalosClient().files.preview.resourceUrl(preview.descriptor)} alt={label} /> : <span role="status">{t("preview.loading")}</span>;
}
function DocumentScroll({ resourcePath, position, onPositionChange, children }: { resourcePath: string; position?: FilePreviewPosition; onPositionChange?: (position: FilePreviewPosition) => void; children: ReactNode }) {
  const element = useRef<HTMLDivElement>(null);
  const saved = useRef(position);
  useEffect(() => { if (element.current && saved.current?.kind === "markdown" && saved.current.resourcePath === resourcePath) element.current.scrollTop = saved.current.scrollTop; }, [resourcePath]);
  return <div ref={element} className="min-h-0 flex-1 overflow-auto px-6 py-6" onScroll={event => {
    if (!event.currentTarget.closest("[hidden], [inert]")) onPositionChange?.({ kind: "markdown", resourcePath, scrollTop: event.currentTarget.scrollTop });
  }}>{children}</div>;
}
/** One resource view for every shell and source. Document transforms and archive
 * members use the same descriptor/read/resource URL contract as source files. */
export function FilePreview({ source, connectionKey, instanceKey, position, onPositionChange }: { source: FilePreviewSource; connectionKey: string; instanceKey: string; position?: FilePreviewPosition; onPositionChange?: (position: FilePreviewPosition) => void }) {
  const { t } = useTranslation("files");
  const preview = useFilePreview(source, connectionKey, instanceKey);
  const descriptor = preview.descriptor;
  const document = descriptor?.document;
  const entries = document?.kind === "archive" ? document.entries : undefined;
  const [selection, setSelection] = useState<{ previewId: string; entry?: FilePreviewEntry }>();
  const selected = selection?.previewId === descriptor?.previewId ? selection?.entry : entries?.find(entry => entry.resourcePath === position?.resourcePath);
  const selectEntry = (entry?: FilePreviewEntry, initialPosition?: FilePreviewPosition) => {
    if (!descriptor) return;
    setSelection({ previewId: descriptor.previewId, entry });
    onPositionChange?.(initialPosition ?? { kind: "archive", resourcePath: entry?.resourcePath ?? null });
  };
  const resourcePath = selected?.resourcePath ?? (document?.kind === "pdf" ? document.resourcePath : descriptor?.entryPath);
  const member = usePreviewMetadata(descriptor, selected?.resourcePath);
  const metadata = (selected ? member.data : undefined) ?? selected?.metadata ?? (document?.kind === "pdf" && descriptor ? { ...descriptor.metadata, mimeType: "application/pdf" } : descriptor?.metadata);
  const file = metadata ? { originalFilename: metadata.name, mimeType: metadata.mimeType, sizeBytes: metadata.sizeBytes } : undefined;
  const display = file ? position?.kind === "text" && (position.resourcePath === null || position.resourcePath === resourcePath) ? { kind: "text" as const } : fileDisplay(file) : null;
  const markdown = display?.kind === "markdown";
  const content = usePreviewContent(descriptor, markdown, resourcePath);
  const [downloadError, setDownloadError] = useState<unknown>();
  const [downloading, setDownloading] = useState(false);
  const error = preview.error ?? (selected ? member.error : null) ?? (markdown ? content.error : null);
  let text: string | null = "";
  if (content.data) { try { text = new TextDecoder("utf-8", { fatal: true }).decode(content.data); } catch { text = null; } }
  const location = selected && descriptor ? `${descriptor.metadata.name} / ${selected.path}` : source.kind === "sessionPath" ? source.path : descriptor?.metadata.name ?? source.fileId;
  const resource = source.kind === "file" ? source : { kind: "sessionFile" as const, sessionId: source.sessionId, path: source.path };
  const download = async () => {
    if (!descriptor) return;
    setDownloadError(undefined); setDownloading(true);
    const path = selected?.resourcePath ?? descriptor.entryPath;
    const downloadMetadata = metadata ?? descriptor.metadata;
    try { downloadFile(await getTalosClient().files.preview.content(descriptor, path), downloadMetadata.name, downloadMetadata.mimeType); }
    catch (failure) { setDownloadError(failure); }
    finally { setDownloading(false); }
  };
  const url = descriptor && resourcePath ? getTalosClient().files.preview.resourceUrl(descriptor, resourcePath) : undefined;
  const surface = useBrowserSurface((!selected || member.isSuccess) && display?.kind === "browser" && display.mimeType !== "application/pdf" ? url : undefined);
  const resolveImageUrl = (reference: string) => {
    if (!descriptor || !resourcePath) return reference;
    const path = getTalosClient().files.preview.resolvePath(resourcePath, reference);
    return path === undefined ? reference : getTalosClient().files.preview.resourceUrl(descriptor, path);
  };
  const openLink = async (reference: string) => {
    if (/^[a-z][a-z\d+.-]*:/i.test(reference) || reference.startsWith("//")) { await openResourceLink(reference); return; }
    const citation = parseFilePreviewReference(reference, { fromPath: resourcePath });
    const path = resourcePath ? citation.path : undefined;
    if (path !== undefined) {
      if (entries) { const next = entries.find(entry => entry.resourcePath === path); if (!next) throw new Error("The archive member is unavailable."); selectEntry(next, citation.line ? { kind: "text", resourcePath: path, line: citation.line, encoding: "utf-8" } : undefined); }
      else if (source.kind === "sessionPath") await openResource({ kind: "sessionFile", sessionId: source.sessionId, path, ...(citation.line ? { position: { kind: "text", resourcePath: null, line: citation.line, encoding: "utf-8" } as const } : {}) });
      else throw new Error("This registered file has no adjacent file source.");
    } else await openResourceLink(reference);
  };
  const loading = <p role="status" className="px-6 py-8 text-sm text-muted">{t("preview.loading")}</p>;
  return <section aria-label={t("preview.title")} className="flex h-full min-h-0 flex-col bg-background">
    <BrowserToolbar surface={surface} source={{ kind: "file", label: `${t("pageLabel")} / ${location}`, resource, preview: selected && descriptor ? { ...descriptor, entryPath: selected.resourcePath, metadata: metadata ?? selected.metadata } : descriptor }} onRefresh={preview.refresh} refreshDisabled={preview.pending || content.isFetching}>
      {file && fileDisplay(file).kind === "markdown" && resourcePath && onPositionChange ? <button type="button" className="shrink-0 rounded-lg px-2 py-1 text-xs text-muted hover:bg-hover" onClick={() => onPositionChange(display?.kind === "text" ? { kind: "markdown", resourcePath, scrollTop: 0 } : { kind: "text", resourcePath, line: 1, encoding: "utf-8" })}>{t(display?.kind === "text" ? "preview.rendered" : "preview.source")}</button> : null}
      <button aria-label={t("preview.download")} title={`${t("preview.download")}: ${file?.originalFilename ?? location}`} disabled={!file || !!error || downloading} onClick={() => void download()} type="button" className="flex size-8 shrink-0 items-center justify-center rounded-full text-muted hover:bg-hover focus-visible:ring-2 focus-visible:ring-brand/40 disabled:opacity-35"><Download aria-hidden="true" size={15} /></button>
    </BrowserToolbar>
    {selected && descriptor ? <ArchiveNavigation name={descriptor.metadata.name} path={selected.path} onBack={() => selectEntry()} /> : null}
    {downloadError ? <p role="alert" className="px-5 py-2 text-xs text-danger">{userVisibleErrorMessage(downloadError, t("preview.downloadFailed"))}</p> : null}
    {error ? <div className="m-5 rounded-xl border border-danger/25 bg-danger/5 p-4">
      <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(error, t("preview.loadFailed"))}</p>
      <button type="button" onClick={preview.refresh} className="mt-3 inline-flex items-center gap-2 rounded-lg border border-border px-3 py-1.5 text-xs focus-visible:ring-2 focus-visible:ring-brand/40"><RotateCw aria-hidden="true" size={13} />{t("preview.retry")}</button>
    </div> : preview.pending || (selected && member.isPending) || (markdown && content.isPending) ? loading
      : entries && !selected ? <ArchiveContents name={descriptor?.metadata.name ?? location} entries={entries} onSelect={selectEntry} />
      : document?.kind === "unavailable" || display?.kind === "unsupported" ? <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center"><FileText aria-hidden="true" size={32} className="text-muted" /><h1 className="max-w-full break-all text-base font-medium">{file?.originalFilename}</h1><p className="text-xs text-muted">{file?.mimeType} · {file ? formatFileSize(file.sizeBytes) : ""}</p><p role="status" className="max-w-xs text-sm text-muted">{t(document?.kind === "unavailable" ? `preview.document.${document.reason}` : "preview.unsupported")}</p></div>
      : display?.kind === "text" && descriptor && resourcePath ? <TextPreview key={`${descriptor.previewId}:${resourcePath}`} descriptor={descriptor} resourcePath={resourcePath} position={position} onPositionChange={onPositionChange} />
      : display?.kind === "browser" && display.mimeType === "application/pdf" && url && resourcePath ? <Suspense fallback={loading}><PdfPreview key={`${descriptor?.previewId}:${resourcePath}`} url={url} resourcePath={resourcePath} position={position} onPositionChange={onPositionChange} onOpenLink={openLink} /></Suspense>
      : display?.kind === "browser" ? <><p className="shrink-0 border-b border-divider px-4 py-1.5 text-[11px] text-muted">{t(display.mimeType === "text/html" ? "preview.htmlHint" : "preview.nativeHint")}</p>{url ? <BrowserFrame key={surface.native ? "native" : url} surface={surface} url={url} title={file?.originalFilename ?? t("preview.title")} /> : loading}</>
      : text === null && descriptor && resourcePath ? <TextPreview key={`${descriptor.previewId}:${resourcePath}`} descriptor={descriptor} resourcePath={resourcePath} position={position} onPositionChange={onPositionChange} />
      : <DocumentScroll key={`${descriptor?.previewId}:${resourcePath}`} resourcePath={resourcePath ?? ""} position={position} onPositionChange={onPositionChange}>{!text?.trim() ? <p className="text-sm text-muted">{t("preview.empty")}</p> : <MarkdownRenderer className="markdown-body prose prose-sm max-w-none break-words" text={text ?? ""} onOpenLink={openLink} resolveImageUrl={resolveImageUrl} renderFileImage={(id, label) => <DocumentImage fileId={id} label={label} scope={connectionKey} instanceKey={instanceKey} />} />}</DocumentScroll>}
  </section>;
}
