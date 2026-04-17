import { Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { Panel } from "@/shared/ui/Panel";
import { FieldBlock, SectionHeader } from "./SectionPrimitives";

export function SettingsAppearanceSection({
  uiTheme,
  setUiTheme,
  desktopNotifications,
  setDesktopNotifications,
}: Pick<
  SettingsPageState,
  "uiTheme" | "setUiTheme" | "desktopNotifications" | "setDesktopNotifications"
>) {
  const { t } = useTranslation(["settings", "common"]);

  return (
    <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
      <SectionHeader
        icon={<Sparkles size={18} />}
        subtitle={t("settings:appearanceDetail")}
        title={t("settings:appearanceTitle")}
      />
      <div className="mt-6 grid gap-4 md:grid-cols-2">
        <FieldBlock label={t("settings:fields.theme")}>
          <select
            className="w-full rounded-2xl border border-border bg-surface/85 px-4 py-3 text-sm outline-none transition focus:border-brand/50"
            onChange={(event) => setUiTheme(event.target.value as "dark" | "light")}
            value={uiTheme}
          >
            <option value="dark">{t("common:themeBlack")}</option>
            <option value="light">{t("common:themeWhite")}</option>
          </select>
        </FieldBlock>
        <button
          className={[
            "rounded-[1.2rem] border px-4 py-4 text-left text-sm transition",
            desktopNotifications
              ? "border-brand/40 bg-brand/10 text-brand"
              : "border-border bg-background/45 text-muted",
          ].join(" ")}
          onClick={() => setDesktopNotifications((current) => !current)}
          type="button"
        >
          <div className="font-medium">{t("settings:desktopNotifications")}</div>
          <div className="mt-2 text-xs leading-6">
            {desktopNotifications
              ? t("settings:desktopNotificationsEnabled")
              : t("settings:desktopNotificationsDisabled")}
          </div>
        </button>
      </div>
    </Panel>
  );
}
