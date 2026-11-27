import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

export function ChatSessionMessageActions({
  onOpenFiles,
  onOpenSettings,
  taloMode = false,
}: {
  onOpenFiles?: () => void;
  onOpenSettings?: () => void;
  taloMode?: boolean;
} = {}) {
  const { t } = useTranslation("chat");
  const navigate = useNavigate();

  return (
    <>
      <Button onClick={onOpenFiles ?? (() => navigate("/files"))} variant="secondary">{t("openFiles")}</Button>
      {taloMode ? null : (
        <Button onClick={() => navigate("/settings/personalization")} variant="secondary">{t("openMemory")}</Button>
      )}
      <Button onClick={onOpenSettings ?? (() => navigate("/settings"))} variant="secondary">{t("openSettings")}</Button>
    </>
  );
}

export function ChatSessionErrorPanel({
  detail,
  title,
}: {
  detail?: string;
  title: string;
}) {
  return (
    <Panel className="rounded-[var(--radius-panel)] border-danger/30 bg-danger/5 p-4">
      <div className="text-sm font-medium text-danger">{title}</div>
      {detail ? <div className="mt-1 text-xs leading-5 text-muted">{detail}</div> : null}
    </Panel>
  );
}
