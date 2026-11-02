import { useQuery } from "@tanstack/react-query";
import { Paperclip } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useAppStore } from "@/shared/store/app-store";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { FileImage } from "./FileImage";
import { openResource } from "@/shared/lib/open-resource";

export function MessageAttachments({ fileIds }: { fileIds: string[] }) {
  return <div className="flex flex-col items-start gap-2">{fileIds.map((fileId) => <Attachment fileId={fileId} key={fileId} />)}</div>;
}

function Attachment({ fileId }: { fileId: string }) {
  const { t } = useTranslation("chat");
  const connectionKey = useAppStore((state) => `${state.connection.authToken}|${state.connection.userId}`);
  const { data, error } = useQuery({
    queryKey: ["talos", connectionKey, "file", fileId],
    queryFn: () => getTalosClient().files.read({ fileId }),
    refetchOnWindowFocus: false,
  });
  if (error) return <span className="text-xs text-muted" role="alert">{userVisibleErrorMessage(error, t("attachmentLoadFailed"))}</span>;
  if (!data) return <span aria-label={t("imageLoading")} className="block h-24 w-36 animate-pulse rounded-lg bg-muted/10" role="status" />;
  return data.file.mimeType.startsWith("image/")
    ? <FileImage fileId={fileId} label={data.file.originalFilename} size="thumbnail" />
    : <button type="button" onClick={() => void openResource({ kind: "file", fileId })} className="inline-flex max-w-full items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 py-1.5 text-xs text-muted hover:bg-hover focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand"><Paperclip aria-hidden="true" className="shrink-0" size={13} /><span className="break-all">{data.file.originalFilename}</span></button>;
}
