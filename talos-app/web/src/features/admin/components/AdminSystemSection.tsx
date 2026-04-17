import { useTranslation } from "react-i18next";

import type { AdminPageState } from "../hooks/useAdminPage";
import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

export function AdminSystemSection({ connection }: Pick<AdminPageState, "connection">) {
  const { t } = useTranslation("admin");

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
      <div className="mb-5 text-sm font-medium text-ink">{t("systemPanel.title")}</div>
      <div className="grid gap-4 md:grid-cols-2">
        <div className="rounded-[1.2rem] border border-border/80 bg-background/45 p-4">
          <div className="text-xs uppercase tracking-[0.18em] text-muted">{t("systemPanel.server")}</div>
          <div className="mt-3 space-y-2 text-sm text-muted">
            <div>{t("systemPanel.version")}</div>
            <div>{t("systemPanel.address", { host: new URL(connection.serverUrl).host })}</div>
            <div>{t("systemPanel.uptime")}</div>
          </div>
        </div>
        <div className="rounded-[1.2rem] border border-border/80 bg-background/45 p-4">
          <div className="text-xs uppercase tracking-[0.18em] text-muted">{t("systemPanel.clients")}</div>
          <div className="mt-3 space-y-2 text-sm text-muted">
            <div>{t("systemPanel.webClient")}</div>
            <div>{t("systemPanel.tuiClient")}</div>
          </div>
        </div>
      </div>
      <div className="mt-5 flex flex-wrap items-center gap-3">
        <Button variant="secondary">{t("systemPanel.reloadConfig")}</Button>
        <Button variant="secondary">{t("systemPanel.cleanExpiredSessions")}</Button>
        <Button variant="danger">{t("systemPanel.restartServer")}</Button>
      </div>
    </Panel>
  );
}
