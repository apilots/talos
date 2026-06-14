import { Globe2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { Panel } from "@/shared/ui/Panel";
import { FieldBlock, OverviewCard, SectionHeader } from "./SectionPrimitives";

export function SettingsGeneralSection({
  locale,
  setLocale,
  draft,
  setDraft,
  uiTheme,
}: Pick<SettingsPageState, "locale" | "setLocale" | "draft" | "setDraft" | "uiTheme">) {
  const { t } = useTranslation(["settings", "common"]);

  return (
    <div className="space-y-5">
      <div className="grid gap-4 md:grid-cols-2">
        <OverviewCard
          detail={t("settings:fields.languageDetail")}
          title={t("common:language")}
          value={locale === "zh-CN" ? t("common:zhCn") : t("common:en")}
        />
        <OverviewCard
          detail={t("settings:appearanceDetail")}
          title={t("settings:fields.theme")}
          value={uiTheme === "dark" ? t("common:themeBlack") : t("common:themeWhite")}
        />
      </div>

      <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
        <SectionHeader
          icon={<Globe2 size={18} />}
          subtitle={t("settings:fields.languageDetail")}
          title={t("settings:fields.languageAndRuntime")}
        />
        <div className="mt-6 grid gap-5 md:grid-cols-2">
          <FieldBlock label={t("common:language")}>
            <select
              className="w-full rounded-2xl border border-border bg-surface/85 px-4 py-3 text-sm outline-none transition focus:border-brand/50"
              onChange={(event) => setLocale(event.target.value as "en" | "zh-CN")}
              value={locale}
            >
              <option value="en">{t("common:en")}</option>
              <option value="zh-CN">{t("common:zhCn")}</option>
            </select>
            <p className="mt-2 text-xs text-muted">{t("settings:fields.languageDetail")}</p>
          </FieldBlock>
          <FieldBlock label={t("common:runtimeMode")}>
            <select
              className="w-full rounded-2xl border border-border bg-surface/85 px-4 py-3 text-sm outline-none transition focus:border-brand/50"
              onChange={(event) =>
                setDraft((current) => ({
                  ...current,
                  runtimeMode: event.target.value as typeof current.runtimeMode,
                }))
              }
              value={draft.runtimeMode}
            >
              <option value="local">{t("common:local")}</option>
              <option disabled value="cloud">
                {t("common:cloud")}
              </option>
            </select>
            <p className="mt-2 text-xs text-muted">{t("settings:fields.runtimeDetail")}</p>
          </FieldBlock>
        </div>
      </Panel>

    </div>
  );
}
