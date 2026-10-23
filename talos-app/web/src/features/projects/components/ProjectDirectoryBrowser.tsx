import { ArrowLeft, FolderOpen, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { FsDirectoryEntry, FsDirectorySnapshot } from "@talos/sdk-browser";

import { cn } from "@/shared/lib/cn";

interface ProjectDirectoryBrowserProps {
  snapshot: FsDirectorySnapshot | null;
  pathInput: string;
  error: string | null;
  loading: boolean;
  onPathInputChange: (value: string) => void;
  onLoadDirectory: (path?: string | null) => void;
  onEnterDirectory: (entry: FsDirectoryEntry) => void;
}

export function ProjectDirectoryBrowser({
  snapshot,
  pathInput,
  error,
  loading,
  onPathInputChange,
  onLoadDirectory,
  onEnterDirectory,
}: ProjectDirectoryBrowserProps) {
  const { t } = useTranslation(["chat", "common"]);

  return (
    <>
      <form
        className="flex gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          onLoadDirectory(pathInput);
        }}
      >
        <input
          className="form-control-focus min-w-0 flex-1 rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] placeholder:text-muted"
          onChange={(event) => onPathInputChange(event.target.value)}
          value={pathInput}
        />
        <button
          className="rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] outline-none transition hover:border-brand/50 focus:outline-none"
          disabled={loading}
          type="submit"
        >
          {loading ? t("common:loading") : t("chat:openPath")}
        </button>
        <button
          className="rounded-xl border border-border/50 bg-card px-3 py-2 text-[13px] outline-none transition hover:border-brand/50 focus:outline-none"
          onClick={() => onLoadDirectory(null)}
          type="button"
        >
          {t("chat:openHome")}
        </button>
      </form>

      {error ? (
        <div className="rounded-xl border border-danger/20 bg-danger/5 px-3 py-2 text-[12px] leading-snug text-danger">
          {error}
        </div>
      ) : null}

      <div className="min-h-[220px] flex-1 overflow-y-auto rounded-xl border border-border/45 bg-card p-1">
        {snapshot?.parent ? (
          <button
            className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] text-muted transition hover:bg-hover hover:text-ink"
            onClick={() => onLoadDirectory(snapshot.parent)}
            type="button"
          >
            <ArrowLeft size={13} />
            {t("chat:parentDirectory")}
          </button>
        ) : null}
        {loading ? (
          <div className="px-2 py-10 text-center text-[13px] text-muted">{t("common:loading")}</div>
        ) : snapshot && snapshot.entries.length > 0 ? (
          snapshot.entries.map((entry) => (
            <button
              className="flex w-full items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[13px] transition hover:bg-hover"
              key={entry.path}
              onClick={() => onEnterDirectory(entry)}
              title={entry.is_symlink ? t("chat:symlinkNotSupported") : entry.path}
              type="button"
            >
              <FolderOpen
                className={cn("shrink-0", entry.is_symlink ? "text-danger" : "text-muted")}
                size={13}
              />
              <span className="min-w-0 flex-1 truncate">{entry.name}</span>
              {entry.is_symlink ? <TriangleAlert className="shrink-0 text-danger" size={13} /> : null}
            </button>
          ))
        ) : (
          <div className="px-2 py-10 text-center text-[13px] text-muted">{t("chat:noDirectories")}</div>
        )}
      </div>
    </>
  );
}
