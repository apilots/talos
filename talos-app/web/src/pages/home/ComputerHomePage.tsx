import { useState } from "react";
import { Laptop, Monitor, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { ComputerConnectDialog, ComputerPicker, useComputers } from "@/features/computers";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";

export function ComputerHomePage() {
  const { t } = useTranslation(["computers", "common"]);
  const { list, total, localId, select } = useComputers();
  const navigate = useNavigate();
  const [connecting, setConnecting] = useState(false);
  const empty = list.isSuccess && total === 0;
  const Icon = empty ? Laptop : Monitor;

  return (
    <div className="flex h-full overflow-y-auto px-6 py-12">
      <section className="m-auto w-full max-w-[370px] text-center" aria-label={t("computers:home.title")}>
        {list.isPending ? <p role="status" className="text-sm text-muted">{t("common:loading")}</p>
          : list.isError ? <>
            <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(list.error, t("computers:loadError"))}</p>
            <Button variant="ghost" className="mt-3" onClick={() => void list.refetch()}>{t("common:retryNow")}</Button>
          </> : <>
            <div className="mx-auto mb-5 flex size-12 items-center justify-center rounded-[14px] border border-border/80 bg-sidebar text-muted">
              <Icon size={23} aria-hidden="true" />
            </div>
            <h1 className="text-[22px] font-medium tracking-tight text-ink">
              {t(empty ? "computers:home.firstComputer" : "computers:choose")}
            </h1>
            <p className="mb-6 mt-2 text-sm leading-relaxed text-muted">
              {t(empty ? "computers:home.firstComputerHelp" : "computers:home.chooseHelp")}
            </p>
            {!empty ? <ComputerPicker total={total} localId={localId} onSelect={(id) => { select(id); navigate("/"); }} /> : null}
            <Button variant={empty ? "primary" : "ghost"} className={empty ? "" : "mt-5 text-xs"} onClick={() => setConnecting(true)}>
              <Plus size={16} aria-hidden="true" />{t("computers:add")}
            </Button>
          </>}
      </section>
      {connecting ? <ComputerConnectDialog onClose={() => setConnecting(false)} /> : null}
    </div>
  );
}
