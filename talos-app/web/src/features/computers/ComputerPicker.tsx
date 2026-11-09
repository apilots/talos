import { useState } from "react";
import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { Button } from "@/shared/ui/Button";
import { ComputerItem } from "./ComputerItem";
import { useComputerList } from "./hooks";

export function ComputerPicker({ total, localId, onSelect }: {
  total: number; localId?: string; onSelect: (id: string) => void;
}) {
  const { t } = useTranslation(["computers", "common"]);
  const [search, setSearch] = useState("");
  const list = useComputerList(search.trim());
  const nodes = list.data?.pages.flatMap((page) => page.data).filter(node => node.managedWorkerPhase == null) ?? [];
  const searchLabel = t("computers:home.searchPersonal");

  return (
    <div className="text-left">
      {total > 4 || search ? <label className="mb-3 flex items-center gap-2 rounded-xl border border-border px-3 py-2.5 focus-within:outline focus-within:outline-2 focus-within:outline-brand">
        <Search size={16} aria-hidden="true" className="text-muted" />
        <input type="search" aria-label={searchLabel} placeholder={searchLabel}
          value={search} onChange={(event) => setSearch(event.target.value)}
          className="min-w-0 flex-1 bg-transparent text-sm text-ink outline-none placeholder:text-muted" />
      </label> : null}
      <div className="max-h-64 space-y-1 overflow-y-auto">
        {list.isPending ? <p role="status" className="p-3 text-center text-sm text-muted">{t("common:loading")}</p>
          : list.isError ? <div className="p-3 text-center">
            <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(list.error, t("computers:loadError"))}</p>
            <Button variant="ghost" onClick={() => void list.refetch()}>{t("common:retryNow")}</Button>
          </div> : <>
            {nodes.map((node) => <ComputerItem key={node.nodeId} node={node} local={node.nodeId === localId} onSelect={() => onSelect(node.nodeId)} />)}
            {nodes.length === 0 ? <p role="status" className="p-3 text-center text-sm text-muted">{t("computers:home.emptyPersonal")}</p> : null}
          </>}
      </div>
    </div>
  );
}
