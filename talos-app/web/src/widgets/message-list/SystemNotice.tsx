import type { AppProtocol } from "@talos/sdk-core";
import { Info, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";
import { systemNoticeMessage } from "@/shared/i18n/systemNoticeMessage";

/** Independent product notices remain visible outside turn activity. */
export function SystemNotice({ notice }: { notice: AppProtocol.AgentSystemNotice }) {
  useTranslation();
  const Icon = notice.level === "info" ? Info : TriangleAlert;
  const color = notice.level === "error" ? "text-danger" : "text-muted";
  return (
    <div role={notice.level === "info" ? "status" : "alert"} data-notice-level={notice.level}
      className={`flex items-start gap-2 px-1 py-2 text-sm leading-6 ${color}`}>
      <Icon aria-hidden="true" className="mt-1 shrink-0" size={14} />
      <p className="min-w-0 whitespace-pre-wrap break-words">{systemNoticeMessage(notice)}</p>
    </div>
  );
}
