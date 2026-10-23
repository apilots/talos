import { LoaderCircle } from "lucide-react";
import { useTranslation } from "react-i18next";

export function StatusMessageBlock({
  detail,
  title,
  variant,
}: {
  detail: string;
  title: string;
  variant: "thinking" | "reconnecting" | "cancelling";
}) {
  const { t } = useTranslation("chat");
  const iconClassName =
    variant === "thinking"
      ? "mt-1.5 flex size-7.5 shrink-0 items-center justify-center rounded-[0.95rem] border border-brand/[0.18] bg-brand/[0.06] text-brand md:size-8"
      : "mt-1.5 flex size-7.5 shrink-0 items-center justify-center rounded-[0.95rem] border border-brand/[0.16] bg-background/[0.78] text-brand md:size-8";

  return (
    <div className="flex items-start gap-3.5 md:gap-4">
      <div className={iconClassName}>
        <LoaderCircle className="animate-spin" size={16} />
      </div>
      <div className="flex-1 rounded-[var(--radius-panel)] border border-brand/[0.16] bg-card px-5 py-4 text-ink">
        <div className="text-[11px] uppercase tracking-[0.16em] text-brand/80">{t("assistantLabel")}</div>
        <div className="mt-2 text-sm font-medium text-ink">{title}</div>
        <p className="mt-1.5 text-sm leading-6 text-muted">{detail}</p>
      </div>
    </div>
  );
}
