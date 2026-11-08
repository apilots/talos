import { useState } from "react";
import { Laptop } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";

import { ComputerPicker, TaloPanel, useComputers } from "@/features/computers";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";

export function ComputerHomePage() {
  const { t } = useTranslation(["computers", "common"]);
  const { list, nodes, total, localId, select } = useComputers();
  const location = useLocation();
  const navigate = useNavigate();
  const [showConnectHelp, setShowConnectHelp] = useState(false);
  const tab = location.pathname === "/talo" ? "talo" : "personal";
  // A Talo remains in the Computer directory and has a separate chooser tab.
  const personalTotal = total - nodes.filter(node => node.managedWorkerPhase != null).length;
  const empty = list.isSuccess && personalTotal === 0;

  function selectTab(next: "personal" | "talo") {
    if (next !== tab) navigate(next === "talo" ? "/talo" : "/");
  }

  return (
    <div className="h-full overflow-y-auto px-5 pb-8 pt-10 sm:px-6 sm:pt-16">
      <section className="mx-auto w-full max-w-[512px]" aria-label={t("computers:home.title")}>
        <h1 className="text-[24px] font-semibold tracking-tight text-ink">{t("computers:home.chooser")}</h1>
        <p className="mb-4 mt-1 text-sm leading-6 text-muted">{t("computers:talo.intro")}</p>
        <div role="tablist" aria-label={t("computers:home.chooser")} className="flex gap-6 border-b border-border"
          onKeyDown={(event) => {
            const next = event.key === "Home" ? "personal" : event.key === "End" ? "talo"
              : event.key === "ArrowLeft" || event.key === "ArrowRight" ? (tab === "personal" ? "talo" : "personal")
                : undefined;
            if (!next) return;
            event.preventDefault();
            event.currentTarget.querySelector<HTMLButtonElement>(`#computer-tab-${next}`)?.focus();
            selectTab(next);
          }}>
          {(["personal", "talo"] as const).map((name) => (
            <button key={name} id={`computer-tab-${name}`} type="button" role="tab"
              aria-controls={`computer-panel-${name}`} aria-selected={tab === name} tabIndex={tab === name ? 0 : -1}
              onClick={() => selectTab(name)}
              className="-mb-px border-b-2 border-transparent px-0.5 pb-2.5 pt-1.5 text-sm text-muted hover:text-ink focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand aria-selected:border-ink aria-selected:font-medium aria-selected:text-ink">
              {t(name === "personal" ? "computers:home.personal" : "computers:talo.title")}
            </button>
          ))}
        </div>
        <div id="computer-panel-personal" role="tabpanel" aria-labelledby="computer-tab-personal" hidden={tab !== "personal"} tabIndex={0} className="py-6">
          {tab === "personal" ? (
            list.isPending ? <p role="status" className="text-sm text-muted">{t("common:loading")}</p>
              : list.isError ? <>
                <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(list.error, t("computers:loadError"))}</p>
                <Button type="button" variant="ghost" className="mt-3" onClick={() => void list.refetch()}>{t("common:retryNow")}</Button>
              </> : <>
                {empty ? (
                  <div className="flex items-start gap-3">
                    <span className="flex size-11 shrink-0 items-center justify-center rounded-xl border border-border bg-surface text-accent">
                      <Laptop size={22} aria-hidden="true" />
                    </span>
                    <div>
                      <h2 className="text-base font-semibold text-ink">{t("computers:home.addPersonal")}</h2>
                      <p className="mt-1 text-sm leading-6 text-muted">{t("computers:home.connectHelp")} {t("computers:home.appearsAutomatically")}</p>
                    </div>
                  </div>
                ) : <>
                  <p className="mb-3 text-sm text-muted">{t("computers:home.chooseHelp")}</p>
                  <ComputerPicker total={personalTotal} localId={localId} onSelect={(id) => { select(id); navigate("/"); }} />
                  <Button type="button" variant="ghost" className="mt-3 px-0 underline underline-offset-4" aria-expanded={showConnectHelp}
                    aria-controls="computer-home-connect-help" onClick={() => setShowConnectHelp(open => !open)}>
                    {t("computers:home.addPersonal")}
                  </Button>
                  <p id="computer-home-connect-help" className="mt-2 text-sm leading-6 text-muted" hidden={!showConnectHelp}>
                    {t("computers:home.connectHelp")} {t("computers:home.appearsAutomatically")}
                  </p>
                </>}
              </>
          ) : null}
        </div>
        <div id="computer-panel-talo" role="tabpanel" aria-labelledby="computer-tab-talo" hidden={tab !== "talo"} tabIndex={0}>
          {tab === "talo" ? <TaloPanel /> : null}
        </div>
      </section>
    </div>
  );
}
