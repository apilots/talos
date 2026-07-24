import { useEffect, useMemo, useState } from "react";
import { Save, Trash2, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { FileInfo } from "@talos/sdk-browser";

import { formatFileMeta, formatFileSize } from "../hooks/useFilesPage";
import { Panel } from "@/shared/ui/Panel";

interface FileDetailsPanelProps {
  file: FileInfo;
  isDeleting: boolean;
  isUpdating: boolean;
  onClose: () => void;
  onDelete: (fileId: string) => Promise<unknown>;
  onUpdate: (
    fileId: string,
    tags: string[],
    metadata: Record<string, string>,
  ) => Promise<unknown>;
}

export function parseFileMetadataInput(value: string): Record<string, string> | null {
  if (!value.trim()) {
    return {};
  }
  try {
    const parsed: unknown = JSON.parse(value);
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      return null;
    }
    return Object.values(parsed).every((item) => typeof item === "string")
      ? (parsed as Record<string, string>)
      : null;
  } catch {
    return null;
  }
}

export function FileDetailsPanel({
  file,
  isDeleting,
  isUpdating,
  onClose,
  onDelete,
  onUpdate,
}: FileDetailsPanelProps) {
  const { i18n, t } = useTranslation("files");
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const [tags, setTags] = useState(file.tags.join(", "));
  const [metadataInput, setMetadataInput] = useState(
    JSON.stringify(file.metadata, null, 2),
  );

  useEffect(() => {
    setTags(file.tags.join(", "));
    setMetadataInput(JSON.stringify(file.metadata, null, 2));
  }, [file.fileId, file.metadata, file.tags]);

  const parsedTags = tags
    .split(",")
    .map((tag) => tag.trim())
    .filter(Boolean);
  const parsedMetadata = useMemo(
    () => parseFileMetadataInput(metadataInput),
    [metadataInput],
  );

  return (
    <Panel className="rounded-[1.35rem] border-border/80 bg-card/82 p-5">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h2 className="truncate text-base font-medium text-ink">{file.originalFilename}</h2>
          <p className="mt-1 text-xs text-muted">
            {file.mimeType} · {formatFileSize(file.sizeBytes)} · {formatFileMeta(file.updatedAt, locale)}
          </p>
        </div>
        <button
          aria-label={t("closeDetails")}
          className="rounded-xl p-2 text-muted transition hover:bg-background/70 hover:text-ink"
          onClick={onClose}
          type="button"
        >
          <X size={15} />
        </button>
      </div>

      <div className="mt-5 grid gap-4">
        <label className="text-xs text-muted">
          <span className="mb-1.5 block">{t("tags")}</span>
          <input
            className="form-control-focus w-full rounded-xl border border-border bg-background/60 px-3 py-2 text-sm text-ink"
            onChange={(event) => setTags(event.target.value)}
            placeholder={t("tagsPlaceholder")}
            value={tags}
          />
        </label>
        <label className="text-xs text-muted">
          <span className="mb-1.5 block">{t("metadata")}</span>
          <textarea
            className="form-control-focus min-h-28 w-full resize-y rounded-xl border border-border bg-background/60 px-3 py-2 font-mono text-xs text-ink"
            onChange={(event) => setMetadataInput(event.target.value)}
            placeholder={t("metadataPlaceholder")}
            value={metadataInput}
          />
        </label>
        {parsedMetadata === null ? (
          <p className="text-xs text-danger" role="status">{t("metadataInvalid")}</p>
        ) : null}
        <button
          className="flex items-center justify-center gap-2 justify-self-end rounded-xl border border-brand/30 bg-brand/10 px-4 py-2 text-sm text-brand transition hover:bg-brand/15 disabled:opacity-50"
          disabled={isUpdating || parsedMetadata === null}
          onClick={() => {
            if (parsedMetadata !== null) {
              void onUpdate(file.fileId, parsedTags, parsedMetadata).catch(() => undefined);
            }
          }}
          type="button"
        >
          <Save size={14} />
          {isUpdating ? t("saving") : t("saveChanges")}
        </button>
      </div>

      <div className="mt-5 flex justify-end border-t border-border/60 pt-4">
        <button
          className="flex items-center gap-2 rounded-xl px-3 py-2 text-sm text-danger transition hover:bg-danger/10 disabled:opacity-50"
          disabled={isDeleting}
          onClick={() => {
            if (window.confirm(t("deleteConfirm", { filename: file.originalFilename }))) {
              void onDelete(file.fileId).catch(() => undefined);
            }
          }}
          type="button"
        >
          <Trash2 size={14} />
          {isDeleting ? t("deleting") : t("delete")}
        </button>
      </div>
    </Panel>
  );
}
