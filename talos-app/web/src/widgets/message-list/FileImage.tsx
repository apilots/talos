import { useEffect, useRef, useState } from "react";

import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useAppStore } from "@/shared/store/app-store";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";

export function FileImage({ fileId, label, size = "full" }: { fileId: string; label: string; size?: "thumbnail" | "full" }) {
  const { t } = useTranslation("chat");
  const dialog = useRef<HTMLDialogElement>(null);
  const connection = useAppStore((state) => state.connection);
  const [source, setSource] = useState<string>();
  const [error, setError] = useState<unknown>();
  useEffect(() => {
    let active = true;
    let objectUrl: string | undefined;
    setSource(undefined);
    setError(undefined);
    void getTalosClient().files.content({ fileId }).then(({ file, data }) => {
      if (!active) return;
      objectUrl = URL.createObjectURL(new Blob([new Uint8Array(data).buffer], { type: file.mimeType }));
      setSource(objectUrl);
    }).catch((failure: unknown) => {
      if (active) setError(failure);
    });
    return () => {
      active = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [connection, fileId]);
  if (error) return <p className="text-xs text-muted" role="alert">{userVisibleErrorMessage(error, t("attachmentLoadFailed"))}</p>;
  if (!source) return <span aria-label={t("imageLoading")} className="block h-24 w-36 animate-pulse rounded-lg bg-muted/10" role="status" />;
  return <>
    <button aria-label={t("imagePreview", { filename: label })} className="block max-w-full rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-brand/40" onClick={() => dialog.current?.showModal()} type="button">
      <img src={source} alt={label} className={size === "thumbnail" ? "max-h-28 max-w-[min(100%,12rem)] rounded-lg object-contain" : "max-h-96 max-w-full rounded-lg object-contain"} onError={() => setError(new Error())} />
    </button>
    <dialog aria-label={t("imagePreview", { filename: label })} className="m-auto max-h-[90vh] max-w-[calc(100vw-2rem)] overflow-visible bg-transparent p-0 backdrop:bg-black/70" onClick={(event) => { if (event.target === event.currentTarget) dialog.current?.close(); }} ref={dialog}>
      <button aria-label={t("closeImagePreview")} className="absolute right-2 top-2 flex size-8 items-center justify-center rounded-full bg-black/60 text-white outline-none focus-visible:ring-2 focus-visible:ring-white" onClick={() => dialog.current?.close()} type="button"><X aria-hidden="true" size={18} /></button>
      <img alt={label} className="max-h-[85vh] max-w-[calc(100vw-2rem)] rounded-lg object-contain" src={source} />
    </dialog>
  </>;
}
