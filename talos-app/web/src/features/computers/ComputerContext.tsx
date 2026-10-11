import { Link, useNavigate } from "react-router-dom";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useTranslation } from "react-i18next";
import { userVisibleErrorMessage, apiErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useComputers } from "./hooks";
import { ComputerItem } from "./ComputerItem";

export function ComputerContext() {
  const { t } = useTranslation("computers");
  const computers = useComputers();
  const navigate = useNavigate();
  return <div className="border-b border-border/70 p-2" data-testid="computer-context">
    {computers.selected ? <ComputerItem node={computers.selected} selected local={computers.selectedId === computers.localId} onSelect={() => navigate("/computers")} /> : <p className="px-3 py-2 text-xs text-muted">{getDesktopBridge() ? t("registering") : <Link to="/computers" className="text-brand">{t("choose")}</Link>}</p>}
    {computers.error ? <p role="alert" className="px-3 pb-2 text-xs text-danger">{userVisibleErrorMessage(computers.error, t("loadError"))}</p> : null}
    {computers.runtime?.error ? <p role="alert" className="px-3 pb-2 text-xs text-danger">{apiErrorMessage(computers.runtime.error)}</p> : null}
  </div>;
}
