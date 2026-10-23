import { useCallback, useRef, useState } from "react";
import { Home, MoreHorizontal, MonitorCog } from "lucide-react";
import { Link, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useLogout } from "@/features/auth";
import { SidebarAccountMenu } from "@/widgets/sidebar/SidebarAccountMenu";
import { useAppStore } from "@/shared/store/app-store";
import { ComputerItem } from "./ComputerItem";
import { ComputerOverflowMenu } from "./ComputerOverflowMenu";
import { useComputers } from "./hooks";

export function ComputerRail() {
  const { t } = useTranslation("computers");
  const computers = useComputers();
  const remaining = Math.max(0, computers.total - computers.quickNodes.length);
  const more = t("moreComputers", { count: remaining });
  const allLabel = remaining > 0 ? `${t("all")} · ${more}` : t("all");
  const navigate = useNavigate();
  const logout = useLogout();
  const connection = useAppStore((state) => state.connection);
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => { setOpen(false); trigger.current?.focus(); }, []);
  const select = (id: string) => { computers.select(id); navigate("/"); };
  return <aside aria-label={t("title")} className="relative z-30 flex h-screen flex-col items-center gap-2 bg-surface px-2 py-4">
    <Link to="/" title="Talos" className="mb-3 flex size-10 items-center justify-center rounded-xl text-ink hover:bg-hover"><Home size={21} /></Link>
    {computers.quickNodes.map((node) => <ComputerItem key={node.nodeId} node={node} compact selected={node.nodeId === computers.selectedId} local={node.nodeId === computers.localId} onSelect={() => select(node.nodeId)} />)}
    <div className="relative"><button ref={trigger} type="button" aria-label={allLabel} aria-expanded={open} aria-haspopup="dialog" onClick={() => setOpen(!open)} className="relative flex size-10 items-center justify-center rounded-xl text-muted hover:bg-hover"><MoreHorizontal size={21} />{remaining > 0 ? <span aria-hidden="true" className="absolute -right-1 -top-1 rounded bg-selected px-1 text-[10px] leading-4 tabular-nums">+{remaining}</span> : null}</button>
      {open ? <ComputerOverflowMenu selectedId={computers.selectedId} localId={computers.localId} onSelect={select} onClose={close} /> : null}
    </div>
    <Link to="/computers" aria-label={t("manage")} title={t("manage")} className="flex size-10 items-center justify-center rounded-xl text-muted hover:bg-hover"><MonitorCog size={20} /></Link>
    <div className="mt-auto"><SidebarAccountMenu compact accountType={connection.accountType} authStatus={connection.authStatus} authUsername={connection.authUsername} onLogout={logout} /></div>
  </aside>;
}
