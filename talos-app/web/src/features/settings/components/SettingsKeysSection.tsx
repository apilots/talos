import { CheckCircle2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/shared/ui/Badge";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";
import { SectionHeader } from "./SectionPrimitives";

export function SettingsKeysSection() {
  const { t } = useTranslation("settings");

  return (
    <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
      <SectionHeader
        icon={<CheckCircle2 size={18} />}
        subtitle={t("apiKeysDetail")}
        title={t("apiKeysTitle")}
      />
      <div className="mt-5 space-y-4">
        {["OpenAI", "Anthropic", "Bailian"].map((provider) => (
          <div className="rounded-[1.25rem] border border-border/80 bg-background/45 p-4" key={provider}>
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="font-medium text-ink">{provider}</div>
                <div className="mt-1 text-xs text-muted">{t("localStorage")}</div>
              </div>
              <Badge tone="warning">{t("pending")}</Badge>
            </div>
            <Input className="mt-4" placeholder={t("keyPlaceholder", { provider })} type="password" />
          </div>
        ))}
      </div>
    </Panel>
  );
}
