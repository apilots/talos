import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Panel } from "@/shared/ui/Panel";
import { workerRuntimeClient } from "../api";
import { RuntimeSettingsForm } from "./RuntimeSettingsForm";

export function SettingsRuntimeSection({ scope }: { scope: string }) {
  const { t } = useTranslation("settings", { keyPrefix: "runtimePanel" });
  const queryClient = useQueryClient();
  const queryKey = ["talos", scope, "worker-runtime"] as const;
  const { data, error } = useQuery({
    queryKey,
    queryFn: () => workerRuntimeClient().read(),
  });

  return (
    <Panel className="space-y-5 rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82] p-5">
      <p className="text-sm leading-6 text-muted">{t("description")}</p>
      {error ? (
        <p className="text-sm text-danger" role="alert">{userVisibleErrorMessage(error, t("requestFailed"))}</p>
      ) : !data ? (
        <p className="text-sm text-muted" role="status">{t("loading")}</p>
      ) : (
        <>
          <dl className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 border-b border-divider pb-4 text-sm">
            <div className="flex items-center gap-3">
              <dt className="text-muted">{t("activeBackend")}</dt>
              <dd className="font-medium text-ink">{t(`backend.${data.activeBackend ?? "disabled"}`)}</dd>
            </div>
            {data.restartRequired ? (
              <div className="flex items-center gap-3">
                <dt className="text-muted">{t("savedBackend")}</dt>
                <dd className="font-medium text-ink">{t(`backend.${data.configuredBackend ?? "disabled"}`)}</dd>
              </div>
            ) : null}
          </dl>
          {data.restartRequired ? (
            <p className="text-sm text-muted" role="status">{t("restartRequired")}</p>
          ) : null}
          {data.configuredBackend && data.kubernetes && data.profile && data.timing ? (
            <RuntimeSettingsForm
              dockerSocketPath={data.dockerSocketPath ?? ""}
              key={`${scope}:${data.configuredBackend}`}
              kubernetes={data.kubernetes}
              backend={data.configuredBackend}
              profile={data.profile}
              timing={data.timing}
              onSaved={(settings) => queryClient.setQueryData(queryKey, settings)}
            />
          ) : (
            <p className="text-sm text-muted">{t("operatorSetup")}</p>
          )}
        </>
      )}
    </Panel>
  );
}
