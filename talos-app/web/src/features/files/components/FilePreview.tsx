import { useState } from "react";
import { Download, FileText, RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { BrowserFrame, BrowserToolbar } from "@/features/web-preview/BrowserToolbar";
import { MarkdownRenderer } from "@/widgets/message-list/MarkdownRenderer";
import { fileDisplay } from "../file-preview";
import { useFileMetadata } from "../hooks/useFileMetadata";
import { downloadFile, useFileContent, useFileObjectUrl } from "../hooks/useFileContent";
import { formatFileSize } from "../hooks/useFilesPage";

function DocumentImage({ fileId, label, scope, instanceKey }: { fileId: string; label: string; scope: string; instanceKey: string }) {
  const { t } = useTranslation("files");
  const content = useFileContent({ scope, fileId, instanceKey, enabled: true });
  const url = useFileObjectUrl(content.data?.data, content.data?.file.mimeType);
  return content.error ? <span role="alert">{userVisibleErrorMessage(content.error, t("preview.loadFailed"))}</span>
    : url ? <img src={url} alt={label} /> : <span role="status">{t("preview.loading")}</span>;
}

/** Registered file preview. Content rendering shares browser chrome, ownership and authenticated APIs. */
export function FilePreview({ fileId, connectionKey, instanceKey }: { fileId: string; connectionKey: string; instanceKey: string }) {
  const { t } = useTranslation("files");
  const metadata = useFileMetadata(connectionKey, fileId);
  const file = metadata.data;
  const display = file ? fileDisplay(file) : null;
  const readable = display && !["unsupported", "tooLarge"].includes(display.kind);
  const content = useFileContent({ scope: connectionKey, fileId, instanceKey, enabled: metadata.isSuccess && !!readable });
  const objectUrl = useFileObjectUrl(readable ? content.data?.data : undefined, display?.kind === "browser" ? display.mimeType : undefined);
  const [revision, setRevision] = useState(0);
  const [downloadError, setDownloadError] = useState<unknown>();
  const [downloading, setDownloading] = useState(false);
  const error = metadata.error ?? (readable ? content.error : null);
  let text: string | null = "";
  if (content.data && (display?.kind === "text" || display?.kind === "markdown")) {
    try { text = new TextDecoder("utf-8", { fatal: true }).decode(content.data.data); } catch { text = null; }
  }
  const refresh = async () => {
    const result = await metadata.refetch();
    if (result.isSuccess && !["unsupported", "tooLarge"].includes(fileDisplay(result.data).kind)) await content.refetch();
    setRevision(value => value + 1);
  };
  const download = async () => {
    if (!file) return;
    setDownloadError(undefined); setDownloading(true);
    try {
      const result = content.data ?? await getTalosClient().files.content({ fileId });
      downloadFile(result.data, result.file.originalFilename, result.file.mimeType);
    } catch (failure) { setDownloadError(failure); }
    finally { setDownloading(false); }
  };
  return <section aria-label={t("preview.title")} className="flex h-full min-h-0 flex-col bg-background">
    <BrowserToolbar source={{ kind: "file", label: `${t("pageLabel")} / ${file?.originalFilename ?? fileId}`, fileId }} onRefresh={() => void refresh()} refreshDisabled={metadata.isFetching || content.isFetching}>
      <button aria-label={t("preview.download")} title={t("preview.download")} disabled={!file || !!error || downloading} onClick={() => void download()} type="button" className="flex size-8 shrink-0 items-center justify-center rounded-full text-muted hover:bg-hover focus-visible:ring-2 focus-visible:ring-brand/40 disabled:opacity-35"><Download aria-hidden="true" size={15} /></button>
    </BrowserToolbar>
    {downloadError ? <p role="alert" className="px-5 py-2 text-xs text-danger">{userVisibleErrorMessage(downloadError, t("preview.downloadFailed"))}</p> : null}
    {error ? <div className="m-5 rounded-xl border border-danger/25 bg-danger/5 p-4">
      <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(error, t("preview.loadFailed"))}</p>
      <button type="button" onClick={() => void refresh()} className="mt-3 inline-flex items-center gap-2 rounded-lg border border-border px-3 py-1.5 text-xs focus-visible:ring-2 focus-visible:ring-brand/40"><RotateCw aria-hidden="true" size={13} />{t("preview.retry")}</button>
    </div> : metadata.isPending || (readable && content.isPending) ? <p role="status" className="px-6 py-8 text-sm text-muted">{t("preview.loading")}</p>
      : display?.kind === "unsupported" || display?.kind === "tooLarge" ? <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center">
        <FileText aria-hidden="true" size={32} className="text-muted" /><h1 className="max-w-full break-all text-base font-medium">{file?.originalFilename}</h1>
        <p className="text-xs text-muted">{file?.mimeType} · {file ? formatFileSize(file.sizeBytes) : ""}</p>
        <p role="status" className="max-w-xs text-sm text-muted">{t(display.kind === "tooLarge" ? "preview.tooLarge" : "preview.unsupported")}</p>
      </div> : display?.kind === "browser" ? <>
        <p className="shrink-0 border-b border-divider px-4 py-1.5 text-[11px] text-muted">{t(display.mimeType === "text/html" ? "preview.htmlHint" : "preview.nativeHint")}</p>
        {objectUrl ? <BrowserFrame key={`${objectUrl}:${revision}`} url={objectUrl} title={file?.originalFilename ?? t("preview.title")} /> : <p role="status" className="p-6 text-sm text-muted">{t("preview.loading")}</p>}
      </> : text === null ? <p role="alert" className="px-6 py-8 text-sm text-danger">{t("preview.invalidEncoding")}</p>
        : <div className="min-h-0 flex-1 overflow-auto px-6 py-6">
          {!text.trim() ? <p className="text-sm text-muted">{t("preview.empty")}</p> : display?.kind === "markdown"
            ? <MarkdownRenderer className="markdown-body prose prose-sm max-w-none break-words" text={text} renderFileImage={(id, label) => <DocumentImage fileId={id} label={label} scope={connectionKey} instanceKey={instanceKey} />} />
            : <pre className="whitespace-pre-wrap break-words font-mono text-xs leading-6 text-ink">{text}</pre>}
        </div>}
  </section>;
}
