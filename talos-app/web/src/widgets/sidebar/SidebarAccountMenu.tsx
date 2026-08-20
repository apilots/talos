import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { LogOut, Settings2, UserCircle } from "lucide-react";
import { NavLink } from "react-router-dom";
import type { AccountType } from "@/shared/types/account";

interface SidebarAccountMenuProps {
  accountType: AccountType;
  authStatus: "unknown" | "loading" | "authenticated" | "unauthenticated" | "logged_out" | "open";
  authUsername: string;
  onLogout: () => Promise<void> | void;
}

export function SidebarAccountMenu({
  accountType,
  authStatus,
  authUsername,
  onLogout,
}: SidebarAccountMenuProps) {
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const accountLabel = authStatus === "open" ? "Open" : accountType;
  const avatarLabel = authUsername.trim().charAt(0).toUpperCase() || "U";

  useEffect(() => {
    if (!open) {
      return;
    }
    function handlePointerDown(event: MouseEvent) {
      if (!menuRef.current?.contains(event.target as Node)) {
        setOpen(false);
      }
    }
    function handleEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
      }
    }
    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleEscape);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleEscape);
    };
  }, [open]);

  return (
    <div className="mt-4 border-t border-border/80 pt-4">
      <div className="relative" ref={menuRef}>
        {open ? (
          <AccountPopover
            accountLabel={accountLabel}
            authStatus={authStatus}
            authUsername={authUsername}
            onClose={() => setOpen(false)}
            onLogout={onLogout}
          />
        ) : null}
        <button
          aria-expanded={open}
          className="flex w-full items-center gap-3 rounded-[0.95rem] px-3 py-2 text-left transition hover:bg-selected/60 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
          onClick={() => setOpen((value) => !value)}
          type="button"
        >
          <div className="flex size-8 shrink-0 items-center justify-center overflow-hidden rounded-full bg-background/85 text-xs font-semibold text-ink">
            {avatarLabel}
          </div>
          <div className="min-w-0">
            <div className="truncate text-[13px] font-medium leading-4 text-ink">{authUsername}</div>
            <div className="mt-0.5 text-[11px] leading-4 text-muted">{accountLabel}</div>
          </div>
        </button>
      </div>
    </div>
  );
}

function AccountPopover({
  accountLabel,
  authStatus,
  authUsername,
  onClose,
  onLogout,
}: {
  accountLabel: string;
  authStatus: SidebarAccountMenuProps["authStatus"];
  authUsername: string;
  onClose: () => void;
  onLogout: SidebarAccountMenuProps["onLogout"];
}) {
  return (
    <div className="absolute bottom-[calc(100%+0.65rem)] left-0 right-0 z-30 overflow-hidden rounded-[1.1rem] border border-border bg-card px-3 py-2 shadow-panel">
      <AccountMenuSection>
        <MenuStaticRow icon={<UserCircle aria-hidden="true" size={16} />} label={authUsername} muted />
        <MenuStaticRow icon={<Settings2 aria-hidden="true" size={16} />} label={`${accountLabel} account`} muted />
      </AccountMenuSection>
      <AccountMenuSection>
        <MenuLink icon={<UserCircle aria-hidden="true" size={16} />} label="Profile" onClose={onClose} to="/settings" />
        <MenuLink icon={<Settings2 aria-hidden="true" size={16} />} label="Settings" onClose={onClose} to="/settings/general" />
      </AccountMenuSection>
      <div className="py-1.5">
        {authStatus !== "open" ? (
          <button
            className="flex h-9 w-full items-center gap-3 rounded-[var(--radius-control)] px-2 text-left text-sm text-ink transition hover:bg-background/70 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
            onClick={() => {
              onClose();
              void onLogout();
            }}
            type="button"
          >
            <LogOut aria-hidden="true" size={16} />
            <span className="min-w-0 flex-1 truncate">Log out</span>
          </button>
        ) : null}
      </div>
    </div>
  );
}

function AccountMenuSection({ children }: { children: ReactNode }) {
  return <div className="border-b border-border/70 py-1.5">{children}</div>;
}

function MenuStaticRow({
  icon,
  label,
  muted = false,
}: {
  icon: ReactNode;
  label: string;
  muted?: boolean;
}) {
  return (
    <div className="flex h-9 items-center gap-3 px-2 text-sm">
      <span className="text-muted">{icon}</span>
      <span className={muted ? "min-w-0 flex-1 truncate text-muted" : "min-w-0 flex-1 truncate text-ink"}>{label}</span>
    </div>
  );
}

function MenuLink({
  icon,
  label,
  onClose,
  to,
}: {
  icon: ReactNode;
  label: string;
  onClose: () => void;
  to: string;
}) {
  return (
    <NavLink
      className="flex h-9 items-center gap-3 rounded-[var(--radius-control)] px-2 text-sm text-ink transition hover:bg-background/70 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
      onClick={onClose}
      to={to}
    >
      <span className="text-muted">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
    </NavLink>
  );
}
