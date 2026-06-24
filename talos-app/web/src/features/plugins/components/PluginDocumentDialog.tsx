import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Loader2, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Plugin } from "@talos/sdk-browser";

import { readPluginDocument } from "@/features/plugins/api";
import {
  marketplaceStatusLabelKey,
  pluginDocumentName,
  pluginTitle,
} from "@/features/plugins/lib/pluginDisplay";
import { PluginDocumentBody } from "./PluginDocumentBody";
import { PluginIconBadge } from "./PluginIconBadge";

interface PluginDocumentDialogProps {
  plugin: Plugin | null;
  onClose: () => void;
}

export function PluginDocumentDialog({ plugin, onClose }: PluginDocumentDialogProps) {
  const { t } = useTranslation("plugins");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const documentQuery = useQuery({
    enabled: Boolean(plugin),
    queryKey: ["talos", "plugins", "document", plugin?.id],
    queryFn: () => readPluginDocument(plugin as Plugin),
  });

  useEffect(() => {
    if (!plugin) {
      return;
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose, plugin]);

  useEffect(() => {
    setSelectedPath(null);
  }, [plugin?.id]);

  if (!plugin) {
    return null;
  }

  const readPlugin = documentQuery.data?.plugin ?? plugin;
  const title = pluginTitle(readPlugin);
  const documentName = pluginDocumentName(plugin, t("documents.pluginDocument"));
  const documents = documentQuery.data?.documents ?? [];
  const selectedDocument = documents.find((document) => document.path === selectedPath) ?? documents[0];

  return (
    <div className="fixed inset-y-0 left-0 right-0 z-[80] flex items-center justify-center bg-ink/28 px-6 py-8 backdrop-blur-[2px] lg:left-[280px]">
      <button
        aria-label={t("dialog.closePluginBackdrop")}
        className="absolute inset-0 cursor-default"
        onClick={onClose}
        type="button"
      />
      <section className="relative flex max-h-[86vh] w-full max-w-[860px] flex-col overflow-hidden rounded-[1.1rem] border border-border bg-background shadow-[0_24px_80px_rgba(34,28,20,0.22)]">
        <header className="flex items-start justify-between gap-4 border-b border-border px-6 py-5">
          <div className="flex min-w-0 items-start gap-3">
            <PluginIconBadge fallback="cloud" plugin={readPlugin} />
            <div className="min-w-0">
              <h2 className="truncate text-lg font-semibold text-ink">{title}</h2>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted">
                <span className="rounded-full bg-card px-2 py-0.5">
                  {t(`status.${marketplaceStatusLabelKey(plugin)}`)}
                </span>
                <span className="min-w-0 truncate">
                  {plugin.path_in_source ?? plugin.metadata.name}
                </span>
              </div>
            </div>
          </div>
          <button
            aria-label={t("dialog.closePlugin")}
            className="flex size-9 shrink-0 items-center justify-center rounded-[0.7rem] text-muted transition hover:bg-card hover:text-ink"
            onClick={onClose}
            type="button"
          >
            <X size={18} />
          </button>
        </header>
        <div className="min-h-[340px] overflow-y-auto px-6 py-5">
          {documentQuery.isLoading ? (
            <div className="flex h-40 items-center justify-center gap-2 text-sm text-muted">
              <Loader2 className="animate-spin" size={16} />
              {t("dialog.loadingDocument", { document: documentName })}
            </div>
          ) : documentQuery.isError ? (
            <div className="rounded-[0.8rem] border border-border bg-card px-4 py-3 text-sm text-muted">
              {t("dialog.failedLoadDocument", { document: documentName })}
            </div>
          ) : (
            <>
              {documents.length > 1 && selectedDocument ? (
                <div className="mb-4 flex flex-wrap gap-2">
                  {documents.map((document) => (
                    <button
                      className={
                        document.path === selectedDocument.path
                          ? "rounded-full bg-ink px-3 py-1.5 text-xs font-medium text-background"
                          : "rounded-full border border-border/80 px-3 py-1.5 text-xs font-medium text-muted transition hover:border-brand/35 hover:text-ink"
                      }
                      key={document.path}
                      onClick={() => setSelectedPath(document.path)}
                      type="button"
                    >
                      {document.title}
                    </button>
                  ))}
                </div>
              ) : null}
              {selectedDocument ? (
                <PluginDocumentBody document={selectedDocument} />
              ) : (
                <div className="rounded-[0.8rem] border border-border bg-card px-4 py-3 text-sm text-muted">
                  {t("dialog.noPluginDocuments")}
                </div>
              )}
            </>
          )}
        </div>
      </section>
    </div>
  );
}
