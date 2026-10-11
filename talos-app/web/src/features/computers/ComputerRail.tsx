import { useCallback, useRef, useState } from "react";
import { MoreHorizontal, MonitorCog, PanelLeft } from "lucide-react";
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
  const navigate = useNavigate();
  const logout = useLogout();
  const connection = useAppStore((state) => state.connection);
  const setSidebarOpen = useAppStore((state) => state.setSidebarOpen);
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => { setOpen(false); trigger.current?.focus(); }, []);
  const select = (id: string) => { computers.select(id); navigate("/"); };
  return <aside aria-label={t("title")} className="relative z-30 flex h-screen flex-col items-center gap-2 border-r border-border bg-sidebar px-2 py-4">
    <Link to="/" title="Talos" className="mb-3 flex size-10 items-center justify-center rounded-xl bg-brand/10 text-xl font-semibold text-brand">T</Link>
    {computers.quickNodes.map((node) => <ComputerItem key={node.nodeId} node={node} compact selected={node.nodeId === computers.selectedId} local={node.nodeId === computers.localId} onSelect={() => select(node.nodeId)} />)}
    <div className="relative"><button ref={trigger} type="button" aria-label={t("all")} aria-expanded={open} aria-haspopup="dialog" onClick={() => setOpen(!open)} className="relative flex size-10 items-center justify-center rounded-xl text-muted hover:bg-selected"><MoreHorizontal size={21} />{computers.total > 3 ? <span className="absolute -right-1 -top-1 rounded bg-selected px-1 text-[9px]">{computers.total - 3}</span> : null}</button>
      {open ? <ComputerOverflowMenu selectedId={computers.selectedId} localId={computers.localId} onSelect={select} onClose={close} /> : null}
    </div>
    <button type="button" className="mt-2 flex size-10 items-center justify-center rounded-xl text-muted lg:hidden" aria-label={t("sidebar")} aria-controls="talos-sidebar" onClick={() => setSidebarOpen(true)}><PanelLeft size={18} /></button>
    <div className="mt-auto"><Link to="/computers" aria-label={t("manage")} title={t("manage")} className="flex size-10 items-center justify-center rounded-xl text-muted hover:bg-selected"><MonitorCog size={20} /></Link>
      <SidebarAccountMenu compact accountType={connection.accountType} authStatus={connection.authStatus} authUsername={connection.authUsername} onLogout={logout} /></div>
  </aside>;
}
