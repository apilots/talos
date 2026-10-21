import { useLocation } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useSessionQuery } from "@/features/chat";
import { ComputerDesktopButton, useComputer, useSelectedComputerId } from "@/features/computers";
import { sidebarInteractiveRowClass } from "@/widgets/sidebar/sidebarRowStyles";

/** Session routes retain their bound Computer instead of following rail selection. */
export function SidebarComputerEntry() {
  const { t } = useTranslation("computers");
  const location = useLocation();
  const sessionId = location.pathname.startsWith("/sessions/") ? location.pathname.slice("/sessions/".length) : "";
  const session = useSessionQuery(sessionId, false);
  const selected = useSelectedComputerId();
  const nodeId = sessionId ? session.data?.nodeId : selected;
  const computer = useComputer(nodeId);
  return <ComputerDesktopButton nodeId={nodeId} online={Boolean(computer.data?.available)} remoteOnly label={t("desktop.yourComputer")} className={`${sidebarInteractiveRowClass} w-full justify-start gap-3 px-3 py-2.5 text-left text-sm font-normal sm:px-3`} />;
}
