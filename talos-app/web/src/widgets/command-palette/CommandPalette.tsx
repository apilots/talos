import { Search } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Panel } from "@/shared/ui/Panel";

export function CommandPalette() {
  const { t } = useTranslation("common");
  return (
    <Panel className="rounded-[1.25rem] border-border/80 bg-card/88 p-4">
      <div className="flex items-center gap-3 text-sm text-muted">
        <Search size={16} />
        <span>{t("commandPalettePlaceholder")}</span>
      </div>
    </Panel>
  );
}
