import { Globe2, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";

import { cn } from "@/shared/lib/cn";
import { useWorkspaceWebpageCreation } from "./desktop-browser";

export function OpenWebpageButton({ className, icon = "plus" }: { className?: string; icon?: "plus" | "globe" }) {
  const { t } = useTranslation("webPreview");
  const creation = useWorkspaceWebpageCreation();
  const label = t("newTab");
  const Icon = icon === "plus" ? Plus : Globe2;

  return <button aria-label={label} title={creation.error ? t("browserFailed") : label} disabled={creation.disabled} className={cn("flex size-8 shrink-0 items-center justify-center rounded-lg text-muted outline-none transition hover:bg-hover hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/40 disabled:opacity-35", className)} onClick={creation.create} type="button"><Icon aria-hidden="true" size={16} /></button>;
}
