import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { AppProtocol } from "@talos/sdk-browser";

import { useProductScope, useSelectedComputerId } from "@/features/computers";
import { sessionMessageSendPolicyOptions, updateSessionMessageSendPolicy } from "@/shared/api/session-message-send-policy";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Panel } from "@/shared/ui/Panel";
import { selectionClasses } from "@/shared/ui/controlStyles";

const policies = ["queue", "steer"] as const;

export function SettingsComposerSection({ canManageConfig }: { canManageConfig: boolean }) {
  const { t } = useTranslation(["settings", "common"]);
  const nodeId = useSelectedComputerId();
  const scope = useProductScope();
  const queryClient = useQueryClient();
  const options = sessionMessageSendPolicyOptions(scope, nodeId);
  const policy = useQuery(options);
  const update = useMutation({
    mutationFn: ({ nodeId, value }: { nodeId: string; value: AppProtocol.SessionMessageSendPolicy }) =>
      updateSessionMessageSendPolicy(nodeId, value),
  });
  const error = (update.variables?.nodeId === nodeId ? update.error : null) ?? policy.error;

  return (
    <section aria-labelledby="settings-composer-heading">
      <h2 id="settings-composer-heading" className="mb-2 text-sm font-medium text-ink">{t("settings:composer.title")}</h2>
      <Panel className="rounded-[var(--radius-panel)] border-border/80 bg-card/[0.82] px-4 py-3">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div id="settings-follow-up-label" className="text-sm font-medium text-ink">{t("settings:composer.followUp")}</div>
            <p className="mt-0.5 text-sm text-muted">{t("settings:composer.followUpDetail")}</p>
          </div>
          <div aria-labelledby="settings-follow-up-label" className="flex shrink-0 gap-1" role="group">
            {policies.map((value) => (
              <button
                aria-pressed={policy.data === value}
                className={`rounded-full px-3 py-1.5 text-sm transition disabled:opacity-60 ${policy.data === value ? selectionClasses.selected : selectionClasses.idle}`}
                disabled={!canManageConfig || !nodeId || !policy.data || update.isPending}
                key={value}
                onClick={() => {
                  if (!nodeId || value === policy.data) return;
                  update.mutate({ nodeId, value }, {
                    onSuccess: (saved) => { queryClient.setQueryData(options.queryKey, saved); },
                  });
                }}
                type="button"
              >
                {t(`settings:composer.${value}`)}
              </button>
            ))}
          </div>
        </div>
        {error ? <p className="mt-2 text-sm text-danger" role="alert">{userVisibleErrorMessage(error, t("settings:requestFailed"))}</p> : null}
      </Panel>
    </section>
  );
}
