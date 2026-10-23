import type { ReactNode } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Panel } from "@/shared/ui/Panel";
import { cn } from "@/shared/lib/cn";

interface CommandResponseSurfaceProps {
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  icon: ReactNode;
  label: string;
  onDismiss?: () => void;
  role?: "region" | "status" | "alert";
  title: string;
}

/** Provides the shared Talos surface for command responses without owning command behavior. */
export function CommandResponseSurface({
  actions,
  children,
  className,
  icon,
  label,
  onDismiss,
  role = "status",
  title,
}: CommandResponseSurfaceProps) {
  const { t } = useTranslation("common");

  return (
    <Panel
      aria-label={label}
      className={cn(
        "mb-2 overflow-hidden rounded-[var(--radius-panel)] border-border/80 bg-card shadow-panel",
        "motion-safe:animate-[command-response-in_160ms_ease-out]",
        className,
      )}
      role={role}
    >
      <div className="flex min-h-11 items-center justify-between gap-4 border-b border-border/65 px-4 py-2.5">
        <div className="flex min-w-0 items-center gap-2.5 text-[13px] font-semibold text-ink">
          <span className="flex size-6 shrink-0 items-center justify-center rounded-[0.65rem] border border-brand/[0.18] bg-brand/[0.07] text-brand">
            {icon}
          </span>
          <span className="truncate">{title}</span>
        </div>
        {actions || onDismiss ? (
          <div className="flex shrink-0 items-center gap-3">
            {actions}
            {onDismiss ? (
              <button
                aria-label={t("close")}
                className="flex size-7 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-hover hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
                onClick={onDismiss}
                type="button"
              >
                <X aria-hidden="true" size={14} />
              </button>
            ) : null}
          </div>
        ) : null}
      </div>
      {children}
    </Panel>
  );
}
