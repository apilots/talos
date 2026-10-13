import { useState } from "react";
import { useTranslation } from "react-i18next";
import { createElectronAppServerClient } from "@talos/sdk-app-server/desktop";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useAppStore } from "@/shared/store/app-store";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";

/** Explicit host mode change; an unreachable cloud never changes mode automatically. */
export function DesktopConnectionControl() {
  const { t } = useTranslation("computers");
  const bridge = getDesktopBridge();
  const mode = useAppStore((state) => state.connection.desktopConnectionMode);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<unknown>();
  if (!bridge) return null;
  return <div className="mt-4 text-sm">
    <button disabled={pending} className="text-brand underline disabled:opacity-50" onClick={() => {
      setPending(true); setError(undefined);
      void createElectronAppServerClient(bridge).desktop.setConnection({ connectionMode: mode === "local" ? "cloud" : "local" })
        .then(() => window.location.reload()).catch((caught: unknown) => { setError(caught); setPending(false); });
    }}>{t(mode === "local" ? "cloudMode" : "localMode")}</button>
    {error ? <p role="alert" className="mt-2 text-danger">{userVisibleErrorMessage(error, t("loadError"))}</p> : null}
  </div>;
}
