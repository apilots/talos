import { useMemo, useState } from "react";
import { useInfiniteQuery } from "@tanstack/react-query";
import type { AppProtocol } from "@talos/sdk-core";
import { ChevronRight, FileText, Image as ImageIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import { fileListQueryKey } from "@/features/files/api";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { cn } from "@/shared/lib/cn";
import { useAppStore } from "@/shared/store/app-store";
import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";
import { useProductScope } from "./hooks";

type FileFilter = "all" | "files" | "media";

const rowFocusClass = "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-brand/40";

function isMediaMime(mimeType: string) {
  return mimeType.startsWith("image/") || mimeType.startsWith("video/") || mimeType.startsWith("audio/");
}

export function TaloConsoleFilesTab({
  active,
  nodeId,
  phase,
  phaseLabel,
}: {
  active: boolean;
  nodeId: string;
  phase: AppProtocol.ManagedWorkerPhase | undefined;
  phaseLabel: string;
}) {
  const { t } = useTranslation(["computers", "common"]);
  const scope = useProductScope();
  const [fileFilter, setFileFilter] = useState<FileFilter>("all");

  const filesQuery = useInfiniteQuery({
    enabled: Boolean(active && nodeId && phase === "ready"),
    initialPageParam: undefined as string | undefined,
    queryKey: [...fileListQueryKey(scope), nodeId, "talo-console"],
    queryFn: async ({ pageParam }) =>
      (await getTalosClient({ service: "files", nodeId })).files.list({
        ...(pageParam ? { cursor: pageParam } : {}),
        limit: 50,
      }),
    getNextPageParam: (page) => page.nextCursor ?? undefined,
    retry: false,
    refetchOnWindowFocus: false,
  });

  const filteredFiles = useMemo(() => {
    const items = filesQuery.data?.pages.flatMap((page) => page.data) ?? [];
    if (fileFilter === "all") return items;
    return items.filter((item) => (fileFilter === "media" ? isMediaMime(item.mimeType) : !isMediaMime(item.mimeType)));
  }, [fileFilter, filesQuery.data?.pages]);

  return (
    <div
      id="talo-console-panel-files"
      role="tabpanel"
      aria-labelledby="talo-console-tab-files"
      className="min-h-0 flex-1 space-y-3.5 overflow-y-auto border-t border-divider px-4 pb-5 pt-4"
    >
      <div className="flex flex-wrap gap-1.5">
        {(["all", "files", "media"] as const).map((entry) => {
          const selected = fileFilter === entry;
          return (
            <button
              key={entry}
              type="button"
              aria-pressed={selected}
              onClick={() => setFileFilter(entry)}
              className={cn(
                "rounded-full border px-3 py-1 text-xs font-medium transition focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand",
                selected
                  ? "border-transparent bg-ink text-background"
                  : "border-border bg-surface text-muted hover:text-ink",
              )}
            >
              {t(
                entry === "all"
                  ? "computers:talo.filterAll"
                  : entry === "files"
                    ? "computers:talo.filterFiles"
                    : "computers:talo.filterMedia",
              )}
            </button>
          );
        })}
      </div>

      {phase !== "ready" ? (
        <p className="py-4 text-center text-xs text-muted">{phaseLabel}</p>
      ) : filesQuery.isLoading ? (
        <p role="status" className="py-4 text-center text-xs text-muted">{t("common:loading")}</p>
      ) : filesQuery.error ? (
        <p role="alert" className="rounded-xl border border-danger/20 bg-danger/5 px-3.5 py-2.5 text-xs leading-5 text-danger">
          {userVisibleErrorMessage(filesQuery.error, t("computers:talo.loadError"))}
        </p>
      ) : filteredFiles.length === 0 ? (
        <p className="py-4 text-center text-xs text-muted">{t("computers:talo.emptyFiles")}</p>
      ) : (
        <div className="space-y-2.5">
          <Panel className="divide-y divide-divider overflow-hidden bg-surface/60 text-xs">
            {filteredFiles.map((file) => {
              const media = isMediaMime(file.mimeType);
              const Icon = media ? ImageIcon : FileText;
              return (
                <button
                  key={file.fileId}
                  type="button"
                  onClick={() =>
                    useAppStore.getState().openWorkspaceWindow({
                      kind: "file",
                      fileId: file.fileId,
                      nodeId,
                    })
                  }
                  className={cn(
                    "flex w-full items-center justify-between gap-2.5 px-3.5 py-2.5 text-left text-ink transition hover:bg-hover",
                    rowFocusClass,
                  )}
                >
                  <span className="flex min-w-0 items-center gap-2.5">
                    <Icon size={14} aria-hidden="true" className="shrink-0 text-muted" />
                    <span className="truncate font-medium">{file.originalFilename}</span>
                  </span>
                  <ChevronRight size={13} aria-hidden="true" className="shrink-0 text-muted" />
                </button>
              );
            })}
          </Panel>
          {filesQuery.hasNextPage ? (
            <div className="flex justify-center">
              <Button
                type="button"
                variant="ghost"
                disabled={filesQuery.isFetchingNextPage}
                onClick={() => void filesQuery.fetchNextPage()}
                className="h-8 rounded-full px-3 text-xs text-muted hover:text-ink"
              >
                {filesQuery.isFetchingNextPage ? t("common:loading") : t("computers:talo.loadMoreFiles")}
              </Button>
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
}
