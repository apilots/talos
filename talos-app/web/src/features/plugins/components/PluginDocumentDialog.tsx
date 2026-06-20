import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { Cloud, Loader2, X } from "lucide-react";
import type { Plugin } from "@talos/sdk-browser";

import { readPluginDocument } from "@/features/plugins/api";
import { MarkdownRenderer } from "@/widgets/message-list/MarkdownRenderer";

interface PluginDocumentDialogProps {
  plugin: Plugin | null;
  onClose: () => void;
}

export function PluginDocumentDialog({ plugin, onClose }: PluginDocumentDialogProps) {
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

  if (!plugin) {
    return null;
  }

  const title = pluginTitle(plugin);
  const contents = stripFrontmatter(documentQuery.data?.contents ?? "");

  return (
    <div className="fixed inset-0 z-[80] flex items-center justify-center bg-ink/28 px-6 py-8 backdrop-blur-[2px]">
      <button
        aria-label="Close plugin document backdrop"
        className="absolute inset-0 cursor-default"
        onClick={onClose}
        type="button"
      />
      <section className="relative flex max-h-[86vh] w-full max-w-[860px] flex-col overflow-hidden rounded-[1.1rem] border border-border bg-background shadow-[0_24px_80px_rgba(34,28,20,0.22)]">
        <header className="flex items-start justify-between gap-4 border-b border-border px-6 py-5">
          <div className="flex min-w-0 items-start gap-3">
            <div className="mt-0.5 flex size-10 shrink-0 items-center justify-center rounded-[0.8rem] bg-card text-brand">
              <Cloud size={18} />
            </div>
            <div className="min-w-0">
              <h2 className="truncate text-lg font-semibold text-ink">{title}</h2>
              <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted">
                <span className="rounded-full bg-card px-2 py-0.5">
                  {pluginStatusLabel(plugin)}
                </span>
                <span className="min-w-0 truncate">{plugin.path_in_source ?? plugin.name}</span>
              </div>
            </div>
          </div>
          <button
            aria-label="Close plugin document"
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
              Loading SKILL.md
            </div>
          ) : documentQuery.isError ? (
            <div className="rounded-[0.8rem] border border-border bg-card px-4 py-3 text-sm text-muted">
              Failed to load SKILL.md.
            </div>
          ) : (
            <MarkdownRenderer
              className="markdown-body prose prose-sm max-w-none prose-headings:font-display prose-p:leading-[1.8]"
              text={contents}
            />
          )}
        </div>
      </section>
    </div>
  );
}

function pluginTitle(plugin: Plugin) {
  if (plugin.definition.kind === "skill") {
    return plugin.definition.plugin.manifest.metadata?.display_name ?? titleFromName(plugin.name);
  }
  return titleFromName(plugin.name);
}

function pluginStatusLabel(plugin: Plugin) {
  switch (plugin.status.status) {
    case "not_installed":
      return "Marketplace";
    case "installed":
      return plugin.status.enabled ? "Installed" : "Disabled";
    case "update_available":
      return "Update available";
    case "source_missing":
      return "Source missing";
    case "conflict":
      return "Conflict";
  }
}

function titleFromName(name: string) {
  return name
    .split(/[-_]/)
    .filter(Boolean)
    .map((part) => `${part.charAt(0).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}

function stripFrontmatter(contents: string) {
  if (!contents.startsWith("---")) {
    return contents;
  }
  const end = contents.indexOf("\n---", 3);
  if (end < 0) {
    return contents;
  }
  return contents.slice(end + 4).trimStart();
}
