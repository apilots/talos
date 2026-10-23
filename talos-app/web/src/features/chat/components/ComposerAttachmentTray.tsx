import {
  CircleAlert,
  FileSpreadsheet,
  FileText,
  Image,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-browser";
import { apiErrorMessage } from "@/shared/i18n/apiErrorMessage";

import type { ComposerAttachmentItem } from "./useComposerAttachments";

interface ComposerAttachmentTrayProps {
  disabled: boolean;
  error: AppProtocol.ApiError | string | null;
  items: ComposerAttachmentItem[];
  onRemove: (id: string) => void;
}

function itemFilename(item: ComposerAttachmentItem) {
  return item.status === "ready" ? item.file.originalFilename : item.file.name;
}

function itemMimeType(item: ComposerAttachmentItem) {
  return item.status === "ready" ? item.file.mimeType : item.file.type;
}

function fileTypeLabel(filename: string) {
  const extension = filename.split(".").at(-1);
  return extension && extension !== filename ? extension.toUpperCase() : "FILE";
}

function AttachmentIcon({ mimeType, filename }: { mimeType: string; filename: string }) {
  if (mimeType.startsWith("image/")) {
    return <Image size={17} />;
  }
  if (/\.(csv|xls|xlsx)$/i.test(filename)) {
    return <FileSpreadsheet size={17} />;
  }
  return <FileText size={17} />;
}

function UploadProgress({
  filename,
  loadedBytes,
  totalBytes,
}: {
  filename: string;
  loadedBytes: number;
  totalBytes: number | null;
}) {
  const { t } = useTranslation("chat");
  const percent =
    totalBytes && totalBytes > 0
      ? Math.min(100, Math.round((loadedBytes / totalBytes) * 100))
      : null;
  const circumference = 44;

  return (
    <svg
      aria-label={t("uploadingAttachment", { filename })}
      aria-valuemax={100}
      aria-valuemin={0}
      aria-valuenow={percent ?? undefined}
      className={percent === null ? "h-6 w-6 animate-spin" : "h-6 w-6 -rotate-90"}
      role="progressbar"
      viewBox="0 0 20 20"
    >
      <circle
        className="stroke-border"
        cx="10"
        cy="10"
        fill="none"
        r="7"
        strokeWidth="2"
      />
      <circle
        className="stroke-brand transition-[stroke-dashoffset] duration-150"
        cx="10"
        cy="10"
        fill="none"
        r="7"
        strokeDasharray={circumference}
        strokeDashoffset={percent === null ? circumference * 0.7 : circumference * (1 - percent / 100)}
        strokeLinecap="round"
        strokeWidth="2"
      />
    </svg>
  );
}

export function ComposerAttachmentTray({
  disabled,
  error,
  items,
  onRemove,
}: ComposerAttachmentTrayProps) {
  const { t } = useTranslation("chat");

  if (items.length === 0 && !error) {
    return null;
  }

  return (
    <div className="px-4 pt-3">
      {items.length > 0 ? (
        <div className="flex flex-wrap gap-2">
          {items.map((item) => {
            const filename = itemFilename(item);
            const mimeType = itemMimeType(item);
            const failure = item.status === "failed" ? apiErrorMessage(item.error) : undefined;
            return (
              <div
                className="flex min-w-0 max-w-full basis-[220px] items-center gap-2.5 rounded-xl border border-border/75 bg-background/55 p-2 pr-2.5"
                key={item.id}
              >
                <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-surface text-brand">
                  {item.status === "failed" ? (
                    <CircleAlert className="text-danger" size={17} />
                  ) : (
                    <AttachmentIcon filename={filename} mimeType={mimeType} />
                  )}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-xs font-medium text-ink" title={filename}>
                    {filename}
                  </span>
                  <span
                    className={`block truncate text-[10px] uppercase tracking-[0.08em] ${
                      item.status === "failed" ? "text-danger" : "text-muted"
                    }`}
                    title={failure}
                  >
                    {item.status === "failed"
                      ? `${t("attachmentUploadFailed")}: ${failure}`
                      : fileTypeLabel(filename)}
                  </span>
                </span>
                {item.status === "uploading" ? (
                  <span className="relative flex h-7 w-7 shrink-0 items-center justify-center">
                    <UploadProgress
                      filename={filename}
                      loadedBytes={item.progress?.loadedBytes ?? 0}
                      totalBytes={item.progress?.totalBytes ?? null}
                    />
                    <button
                      aria-label={t("removeAttachment", { filename })}
                      className="absolute inset-0 flex items-center justify-center rounded-full text-muted transition hover:text-danger"
                      disabled={disabled}
                      onClick={() => onRemove(item.id)}
                      type="button"
                    >
                      <X size={10} />
                    </button>
                  </span>
                ) : (
                  <button
                    aria-label={t("removeAttachment", { filename })}
                    className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-muted transition hover:bg-hover hover:text-danger disabled:cursor-not-allowed disabled:opacity-50"
                    disabled={disabled}
                    onClick={() => onRemove(item.id)}
                    type="button"
                  >
                    <X size={12} />
                  </button>
                )}
              </div>
            );
          })}
        </div>
      ) : null}
      {error ? (
        <div className="pt-2 text-xs text-danger" role="status">
          {typeof error === "string" ? error : apiErrorMessage(error)}
        </div>
      ) : null}
    </div>
  );
}
