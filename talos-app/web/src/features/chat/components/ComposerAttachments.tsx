import { useRef, useState } from "react";
import { FileText, Image, Plus, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { FileInfo } from "@talos/sdk-browser";

import type { ComposerAttachmentKind } from "./useComposerAttachments";

interface ComposerAttachmentsProps {
  availableFiles: FileInfo[];
  disabled: boolean;
  hasMoreAvailable: boolean;
  isLoading: boolean;
  isLoadingMore: boolean;
  supportedKinds: ComposerAttachmentKind[];
  onLoadAvailable: () => Promise<unknown>;
  onLoadMoreAvailable: () => Promise<unknown>;
  onSelect: (file: FileInfo) => void;
  onUpload: (files: readonly File[]) => Promise<unknown>;
}

export function ComposerAttachments({
  availableFiles,
  disabled,
  hasMoreAvailable,
  isLoading,
  isLoadingMore,
  supportedKinds,
  onLoadAvailable,
  onLoadMoreAvailable,
  onSelect,
  onUpload,
}: ComposerAttachmentsProps) {
  const { t } = useTranslation("chat");
  const inputRef = useRef<HTMLInputElement>(null);
  const [menu, setMenu] = useState<"closed" | "actions" | "files">("closed");
  const attachmentsDisabled = disabled || supportedKinds.length === 0;
  const accept = supportedKinds.length === 1 && supportedKinds[0] === "image"
    ? "image/*"
    : undefined;

  return (
    <div className="relative">
      <input
        accept={accept}
        className="hidden"
        multiple
        onChange={(event) => {
          const files = Array.from(event.target.files ?? []);
          event.target.value = "";
          setMenu("closed");
          if (files.length > 0) {
            void onUpload(files).catch(() => undefined);
          }
        }}
        ref={inputRef}
        type="file"
      />
      <button
        aria-label={t("addAttachment")}
        className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-muted outline-none transition hover:bg-hover hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/25 disabled:cursor-not-allowed disabled:opacity-45"
        disabled={attachmentsDisabled}
        onClick={() => setMenu((current) => current === "closed" ? "actions" : "closed")}
        title={supportedKinds.length === 0 ? t("attachmentsUnsupported") : t("addAttachment")}
        type="button"
      >
        <Plus size={18} strokeWidth={1.8} />
      </button>

      {menu !== "closed" ? (
        <div className="absolute bottom-9 left-0 z-50 min-w-60 rounded-xl border border-border bg-card p-1.5 shadow-lg">
          {menu === "actions" ? (
            <>
              <button
                className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-xs text-ink hover:bg-hover"
                onClick={() => inputRef.current?.click()}
                type="button"
              >
                <Upload size={13} />
                {t("uploadAttachment")}
              </button>
              <button
                className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-xs text-ink hover:bg-hover"
                onClick={() => {
                  setMenu("files");
                  void onLoadAvailable().catch(() => undefined);
                }}
                type="button"
              >
                <FileText size={13} />
                {t("chooseExistingFile")}
              </button>
            </>
          ) : isLoading ? (
            <div className="px-3 py-2 text-xs text-muted">{t("loadingAttachments")}</div>
          ) : (
            <div className="max-h-52 overflow-y-auto">
              {availableFiles.length === 0 ? (
                <div className="px-3 py-2 text-xs text-muted">{t("noAvailableAttachments")}</div>
              ) : null}
              {availableFiles.map((file) => (
                <button
                  className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-xs text-ink hover:bg-hover"
                  key={file.fileId}
                  onClick={() => {
                    onSelect(file);
                    setMenu("closed");
                  }}
                  type="button"
                >
                  {file.mimeType.startsWith("image/") ? <Image size={13} /> : <FileText size={13} />}
                  <span className="truncate">{file.originalFilename}</span>
                </button>
              ))}
              {hasMoreAvailable ? (
                <button
                  className="w-full rounded-lg px-3 py-2 text-center text-xs text-muted hover:bg-hover hover:text-ink disabled:opacity-50"
                  disabled={isLoadingMore}
                  onClick={() => void onLoadMoreAvailable().catch(() => undefined)}
                  type="button"
                >
                  {isLoadingMore ? t("loadingAttachments") : t("loadMoreAttachments")}
                </button>
              ) : null}
            </div>
          )}
        </div>
      ) : null}
    </div>
  );
}
