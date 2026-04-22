import { Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { Panel } from "@/shared/ui/Panel";
import { SectionHeader } from "./SectionPrimitives";

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
      <div className="mt-5 divide-y divide-border/60">
        <div className="flex items-center justify-between gap-6 py-4">
          <span className="text-sm text-ink">{t("settings:fields.theme")}</span>
          <select
            className="w-36 rounded-xl border border-border bg-surface/85 px-3 py-2 text-sm outline-none transition focus:border-brand/50"
            onChange={(event) => setUiTheme(event.target.value as "dark" | "light")}
            value={uiTheme}
          >
            <option value="dark">{t("common:themeBlack")}</option>
            <option value="light">{t("common:themeWhite")}</option>
          </select>
        </div>
        <button
          className="flex w-full items-center justify-between gap-6 py-4 text-left"
          onClick={() => setDesktopNotifications((current) => !current)}
          type="button"
        >
          <div>
            <div className="text-sm text-ink">{t("settings:desktopNotifications")}</div>
            <div className="mt-0.5 text-xs text-muted">
              {desktopNotifications
                ? t("settings:desktopNotificationsEnabled")
                : t("settings:desktopNotificationsDisabled")}
            </div>
          </div>
          <div
            className={[
              "shrink-0 rounded-full px-3 py-1 text-xs font-medium transition",
              desktopNotifications
                ? "bg-brand/12 text-brand"
                : "bg-background/60 text-muted",
            ].join(" ")}
          >
            {desktopNotifications ? t("settings:toggleOn") : t("settings:toggleOff")}
          </div>
        </button>
      </div>
    </Panel>
  );
}
