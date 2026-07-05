import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { useConnectionKey } from "@/features/chat";
import { getTalosClient } from "@/shared/api/client";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { FieldBlock } from "../SectionPrimitives";

export function AgentWorkspaceGeneralPanel() {
  const { t } = useTranslation(["settings", "common"]);
  const connectionKey = useConnectionKey();
  const workspaceInfoQuery = useQuery({
    queryKey: ["talos", connectionKey, "workspace-info"],
    queryFn: () => getTalosClient().workspaceDocs.info(),
    refetchOnWindowFocus: false,
  });
  const baseWorkspacePath = workspaceInfoQuery.data?.base_workspace_dir ?? "";
  const userWorkspacePath = workspaceInfoQuery.data?.user_workspace_dir ?? "";

  return (
    <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/90 p-5">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <div className="text-xs uppercase tracking-[0.22em] text-muted">
            {t("settings:agentGeneral")}
          </div>
          <h2 className="mt-2 font-display text-[24px] tracking-[-0.045em] text-ink">
            {t("settings:workspacePathValue")}
          </h2>
          <p className="mt-2 text-sm text-muted">{t("settings:agentWorkspacePathDetail")}</p>
        </div>
      </div>
      <div className="mt-5 max-w-3xl">
        <FieldBlock label={t("settings:baseWorkspace")}>
          <Input disabled value={baseWorkspacePath || t("common:loading")} />
        </FieldBlock>
        <div className="mt-4">
          <FieldBlock label={t("settings:currentUserWorkspace")}>
            <Input disabled value={userWorkspacePath || t("common:loading")} />
          </FieldBlock>
        </div>
        {workspaceInfoQuery.error instanceof Error ? (
          <div className="mt-4 rounded-[var(--radius-control)] border border-danger/20 bg-danger/5 px-3 py-2 text-sm text-danger">
            {workspaceInfoQuery.error.message}
          </div>
        ) : null}
      </div>
    </Panel>
  );
}
