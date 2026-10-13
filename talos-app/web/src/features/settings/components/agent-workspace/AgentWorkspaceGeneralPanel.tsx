import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { useEnvironmentKey } from "@/features/computers";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";

/** Inspect server-owned storage for the current actor's default workspace. */
export function AgentWorkspaceGeneralPanel() {
  const { t } = useTranslation("settings");
  const connectionKey = useEnvironmentKey();
  const query = useQuery({
    queryKey: ["talos", connectionKey, "workspace-info"],
    queryFn: () => getTalosClient().workspaceDocs.info(),
  });

  return (
    <section aria-label={t("agentWorkspace")} className="px-1 py-2">
      <p className="mb-5 text-sm leading-6 text-muted">{t("workspaceInfoDetail")}</p>
      {query.isPending ? <p role="status" className="text-sm text-muted">{t("workspaceInfoLoading")}</p> : null}
      {query.error ? <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(query.error, t("workspaceInfoError"))}</p> : null}
      {query.data ? (
        <dl className="divide-y divide-border/40 text-sm">
          {([
            ["workspaceId", query.data.workspaceId],
            ["workspaceRootPrefix", query.data.rootPrefix],
            ["workspaceUserPath", query.data.userWorkspaceDir],
          ] as const).map(([label, value]) => (
            <div className="grid gap-2 py-3 sm:grid-cols-[10rem_minmax(0,1fr)] sm:gap-4" key={label}>
              <dt className="text-muted">{t(label)}</dt>
              <dd className="min-w-0 select-text break-all font-mono text-xs leading-6 text-ink">{value}</dd>
            </div>
          ))}
        </dl>
      ) : null}
    </section>
  );
}
