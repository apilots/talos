import { useEffect, useRef, useState } from "react";
import { LogOut, Settings2 } from "lucide-react";
import { NavLink } from "react-router-dom";

interface SidebarAccountMenuProps {
  authStatus: "unknown" | "loading" | "authenticated" | "unauthenticated" | "logged_out" | "open";
  authRole: "user" | "admin";
  authUsername: string;
  onLogout: () => Promise<void> | void;
}

export function SidebarAccountMenu({
  authStatus,
  authRole,
  authUsername,
  onLogout,
}: SidebarAccountMenuProps) {
  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const roleLabel = authStatus === "open" ? "Open" : authRole === "admin" ? "Admin" : "Pro";
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
    <div className="relative mt-4 border-t border-border/80 pt-4" ref={menuRef}>
      {open ? (
        <div className="absolute bottom-[calc(100%+0.5rem)] left-0 right-0 z-20 rounded-[1rem] border border-border/80 bg-card p-1.5 shadow-[0_20px_36px_rgba(95,84,64,0.16)]">
          <NavLink
            className="flex w-full items-center gap-2 rounded-[0.85rem] px-3 py-2 text-sm text-ink transition hover:bg-background/75"
            onClick={() => setOpen(false)}
            to="/settings"
          >
            <Settings2 size={14} />
            Settings
          </NavLink>
          {authStatus !== "open" ? (
            <button
              className="flex w-full items-center gap-2 rounded-[0.85rem] px-3 py-2 text-sm text-ink transition hover:bg-background/75"
              onClick={() => {
                setOpen(false);
                void onLogout();
              }}
              type="button"
            >
              <LogOut size={14} />
              Sign out
            </button>
          ) : null}
        </div>
      ) : null}
      <button
        aria-expanded={open}
        className="flex w-full items-center gap-3 rounded-[0.95rem] px-2 py-2 text-left transition hover:bg-card/70"
        onClick={() => setOpen((value) => !value)}
        type="button"
      >
        <div className="flex size-8 shrink-0 items-center justify-center overflow-hidden rounded-full bg-background/85 text-xs font-semibold text-ink">
          {avatarLabel}
        </div>
        <div className="min-w-0">
          <div className="truncate text-[13px] font-medium leading-4 text-ink">{authUsername}</div>
          <div className="mt-0.5 text-[11px] leading-4 text-muted">{roleLabel}</div>
        </div>
      </button>
    </div>
  );
}
