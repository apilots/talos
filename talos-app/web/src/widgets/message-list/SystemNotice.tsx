import type { AppProtocol } from "@talos/sdk-core";
import { useTranslation } from "react-i18next";
import { apiErrorMessage } from "@/shared/i18n/apiErrorMessage";

/** Render both streamed and historical notices without replacing completed output. */
export function SystemNotice({ notice }: { notice: AppProtocol.AgentSystemNotice }) {
  useTranslation();
  const message = notice.content.type === "error" ? apiErrorMessage(notice.content.data) : notice.content.data;
  const color = notice.level === "error" ? "border-danger/30 text-danger" : "border-amber-500/30 text-amber-700 dark:text-amber-300";
  return <div role={notice.level === "info" ? "status" : "alert"} data-notice-level={notice.level}
    className={`whitespace-pre-wrap break-words rounded-xl border px-4 py-3 text-sm leading-6 ${color}`}>{message}</div>;
}
