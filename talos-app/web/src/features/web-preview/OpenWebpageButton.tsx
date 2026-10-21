import { Globe2, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useAppStore } from "@/shared/store/app-store";
import { cn } from "@/shared/lib/cn";

export function OpenWebpageButton({ className, icon = "plus" }: { className?: string; icon?: "plus" | "globe" }) {
  const { t } = useTranslation("webPreview");
  const createWorkspaceWebpage = useAppStore(state => state.createWorkspaceWebpage);
  const label = t("newTab");
  const Icon = icon === "plus" ? Plus : Globe2;

  return <button aria-label={label} title={label} className={cn("flex size-8 shrink-0 items-center justify-center rounded-lg text-muted outline-none transition hover:bg-surface hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/40", className)} onClick={createWorkspaceWebpage} type="button"><Icon aria-hidden="true" size={16} /></button>;
}
