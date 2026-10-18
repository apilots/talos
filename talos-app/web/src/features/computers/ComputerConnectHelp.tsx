import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { createElectronAppServerClient } from "@talos/sdk-app-server/desktop";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";

export function ComputerConnectHelp({ scope }: { scope: string }) {
  const { t } = useTranslation("computers");
  const queries = useQueryClient();
  const reconnect = useMutation({
    mutationFn: async () => {
      const bridge = getDesktopBridge();
      if (bridge) await createElectronAppServerClient(bridge).desktop.enrollWorker({});
    },
    onSuccess: () => queries.invalidateQueries({ queryKey: ["talos", scope] }),
  });
  return (
    <div id="computer-connect-help" className="cmp-notice">
      <p>{t("addHelp")}</p>
      {getDesktopBridge() ? <Button variant="secondary" className="mt-3" disabled={reconnect.isPending} onClick={() => reconnect.mutate()}>{t("recover")}</Button> : null}
      {reconnect.error ? <p role="alert" className="mt-3 text-danger">{userVisibleErrorMessage(reconnect.error, t("loadError"))}</p> : null}
    </div>
  );
}
