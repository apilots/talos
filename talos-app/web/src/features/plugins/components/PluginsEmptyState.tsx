import { Layers3 } from "lucide-react";
import { useTranslation } from "react-i18next";

interface PluginsEmptyStateProps {
  description: string;
}

export function PluginsEmptyState({ description }: PluginsEmptyStateProps) {
  const { t } = useTranslation("plugins");

  return (
    <div className="rounded-[1.25rem] border border-dashed border-border/80 bg-card/55 px-6 py-12 text-center">
      <Layers3 className="mx-auto text-muted" size={28} />
      <div className="mt-4 text-[16px] font-semibold text-ink">{t("panel.noPluginsInstalled")}</div>
      <div className="mx-auto mt-1 max-w-md text-sm leading-6 text-muted">{description}</div>
    </div>
  );
}
