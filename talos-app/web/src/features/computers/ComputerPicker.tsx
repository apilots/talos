import { useState } from "react";
import type { TalosRuntimeState } from "@talos/sdk-app-server/desktop";
import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ComputerDirectoryError } from "./ComputerDirectoryError";
import { ComputerItem } from "./ComputerItem";
import type { AppProtocol } from "@talos/sdk-core";
import { useComputerList } from "./hooks";

export function ComputerPicker({ total, localNode, localRuntime, onSelect }: {
  total: number; localNode?: AppProtocol.NodeInfo; localRuntime?: TalosRuntimeState; onSelect: (id: string) => void;
}) {
  const { t } = useTranslation(["computers", "common"]);
  const [search, setSearch] = useState("");
  const list = useComputerList(search.trim());
  const directory = list.data?.pages.flatMap((page) => page.data).filter(node => node.managedWorkerPhase == null) ?? [];
  const nodes = localNode ? [localNode, ...directory.filter(node => node.nodeId !== localNode.nodeId)] : directory;
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
        {list.isPending ? <p role="status" className="p-3 text-center text-sm text-muted">{t("common:loading")}</p> : null}
        {list.isError ? <div className="p-3 text-center">
          <ComputerDirectoryError error={list.error} pending={list.isFetching} onRetry={() => void list.refetch()} />
        </div> : null}
        {nodes.map((node) => <ComputerItem key={node.nodeId} node={node} local={node.nodeId === localNode?.nodeId} localRuntime={localRuntime} onSelect={() => onSelect(node.nodeId)} />)}
        {!list.isPending && !list.isError && nodes.length === 0 ? <p role="status" className="p-3 text-center text-sm text-muted">{t("computers:home.emptyPersonal")}</p> : null}
      </div>
    </div>
  );
}
