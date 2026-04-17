import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

export function AdminUsersSection() {
  const { t } = useTranslation("admin");
  const userRows = useMemo(
    () => [
      {
        id: "u-1",
        name: "xujiang",
        role: t("usersPanel.roles.admin"),
        status: t("states.active"),
        lastSeen: t("usersPanel.lastSeen.fiveMinutes"),
      },
      {
        id: "u-2",
        name: "alice",
        role: t("usersPanel.roles.member"),
        status: t("states.active"),
        lastSeen: t("usersPanel.lastSeen.twoHours"),
      },
      {
        id: "u-3",
        name: "bob",
        role: t("usersPanel.roles.member"),
        status: t("states.suspended"),
        lastSeen: t("usersPanel.lastSeen.threeDays"),
      },
    ],
    [t],
  );

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-medium text-ink">{t("usersPanel.title")}</div>
          <div className="mt-1 text-sm text-muted">{t("usersPanel.detail")}</div>
        </div>
        <Button variant="secondary">{t("usersPanel.create")}</Button>
      </div>
      <div className="space-y-3">
        {userRows.map((user) => (
          <div
            className="grid grid-cols-[minmax(0,1fr)_140px_140px_140px_120px] items-center gap-4 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-4"
            key={user.id}
          >
            <div className="font-medium text-ink">{user.name}</div>
            <div className="text-sm text-muted">{user.role}</div>
            <div className="text-sm text-muted">{user.status}</div>
            <div className="text-sm text-muted">{user.lastSeen}</div>
            <div className="flex justify-end">
              <Button variant="ghost">{t("usersPanel.edit")}</Button>
            </div>
          </div>
        ))}
      </div>
    </Panel>
  );
}
