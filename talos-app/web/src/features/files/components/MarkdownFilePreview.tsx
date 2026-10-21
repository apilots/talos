import { useQuery } from "@tanstack/react-query";
import { FileText, RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useState, type MouseEvent } from "react";

import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { openExternalUrl } from "@/shared/lib/open-external";
import { normalizeWorkspaceWebpageUrl } from "@/shared/store/workspace-window";
import { MarkdownRenderer } from "@/widgets/message-list/MarkdownRenderer";
import { isMarkdownFile } from "../api";
import { useFileMetadata } from "../hooks/useFileMetadata";

export function MarkdownFilePreview({ fileId, connectionKey }: { fileId: string; connectionKey: string }) {
  const { t } = useTranslation("files");
  const [linkError, setLinkError] = useState(false);
  const metadata = useFileMetadata(connectionKey, fileId);
  const supported = metadata.data ? isMarkdownFile(metadata.data) : false;
  const content = useQuery({
    queryKey: ["talos", connectionKey, "file", fileId, "markdown"],
    queryFn: async () => {
      const response = await getTalosClient().files.content({ fileId });
      try { return new TextDecoder("utf-8", { fatal: true }).decode(response.data); }
      catch { return null; }
    },
    enabled: metadata.isSuccess && supported,
    refetchOnWindowFocus: false,
    retry: false,
  });
  const error = metadata.error ?? content.error;
  const openDocumentLink = (event: MouseEvent<HTMLDivElement>) => {
    const link = event.target instanceof Element ? event.target.closest("a") : null;
    if (!link) return;
    event.preventDefault();
    const url = normalizeWorkspaceWebpageUrl(link.getAttribute("href") ?? "");
    setLinkError(!url);
    if (url) void openExternalUrl(url).catch(() => setLinkError(true));
  };

  return (
    <section aria-label={t("preview.title")} className="flex h-full min-h-0 flex-col bg-card">
      <div className="flex items-center gap-2.5 border-b border-border/70 px-5 py-3">
        <FileText aria-hidden="true" className="shrink-0 text-muted" size={16} />
        <span className="min-w-0 flex-1 truncate text-sm font-medium text-ink">{metadata.data?.originalFilename ?? t("preview.title")}</span>
        <span className="text-[11px] text-muted">{t("preview.readOnly")}</span>
      </div>
      {error ? (
        <div className="m-5 rounded-xl border border-danger/25 bg-danger/5 p-4">
          <p className="text-sm text-danger" role="alert">{userVisibleErrorMessage(error, t("preview.loadFailed"))}</p>
          <button className="mt-3 inline-flex items-center gap-2 rounded-lg border border-border bg-card px-3 py-1.5 text-xs text-ink outline-none hover:bg-background focus-visible:ring-2 focus-visible:ring-brand/40" onClick={() => { void (metadata.error ? metadata.refetch() : content.refetch()); }} type="button">
            <RotateCw aria-hidden="true" size={13} />{t("preview.retry")}
          </button>
        </div>
      ) : metadata.isPending || (supported && content.isPending) ? (
        <p className="px-6 py-8 text-sm text-muted" role="status">{t("preview.loading")}</p>
      ) : !supported ? (
        <p className="px-6 py-8 text-sm text-muted" role="status">{t("preview.unsupported")}</p>
      ) : content.data === null ? (
        <p className="px-6 py-8 text-sm text-danger" role="alert">{t("preview.invalidEncoding")}</p>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto px-6 py-6" onAuxClickCapture={openDocumentLink} onClickCapture={openDocumentLink}>
          {linkError ? <p className="mb-4 text-xs text-danger" role="alert">{t("preview.linkFailed")}</p> : null}
          {content.data?.trim() ? <MarkdownRenderer className="markdown-body prose prose-sm max-w-none break-words" text={content.data} /> : <p className="text-sm text-muted">{t("preview.empty")}</p>}
        </div>
      )}
    </section>
  );
}
