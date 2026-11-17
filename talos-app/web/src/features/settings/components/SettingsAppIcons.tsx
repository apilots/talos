import { useId } from "react";
import { useTranslation } from "react-i18next";

import { AppIcon } from "@/shared/app-icons/AppIcon";
import { APP_ICONS, APP_ICON_IDS } from "@/shared/app-icons/catalog";
import { cn } from "@/shared/lib/cn";
import { useAppStore } from "@/shared/store/app-store";
import { selectionClasses } from "@/shared/ui/controlStyles";

export function SettingsAppIcons() {
  const { t } = useTranslation("settings");
  const groupName = useId();
  const selectedId = useAppStore((state) => state.appIconId);
  const setAppIconId = useAppStore((state) => state.setAppIconId);
  const desktopFailed = useAppStore((state) => state.appIconDesktopFailed);

  return (
    <div className="py-4">
      <fieldset className="min-w-0">
        <legend className="text-sm text-ink">{t("appIcons.title")}</legend>
        <div className="mt-3 grid max-w-xl grid-cols-[repeat(auto-fit,minmax(min(100%,10rem),1fr))] gap-3">
          {APP_ICON_IDS.map((id) => {
            const selected = selectedId === id;
            return (
              <label
                className={cn(
                  "flex min-h-16 cursor-pointer items-center gap-3 rounded-[var(--radius-control)] border p-2.5 text-sm text-ink transition-colors focus-within:ring-2 focus-within:ring-brand/40",
                  selected
                    ? cn(selectionClasses.selected, selectionClasses.border)
                    : "border-border bg-card hover:border-selectedBorder hover:bg-hover",
                )}
                key={id}
              >
                <AppIcon alt="" className="size-10 shrink-0" id={id} />
                <span className="min-w-0 flex-1 font-medium">{t(APP_ICONS[id].labelKey)}</span>
                <input
                  checked={selected}
                  className="size-4 shrink-0 accent-brand"
                  name={groupName}
                  onChange={() => setAppIconId(id)}
                  type="radio"
                />
              </label>
            );
          })}
        </div>
      </fieldset>
      {desktopFailed ? <p className="mt-2 text-sm text-danger" role="alert">{t("appIcons.desktopError")}</p> : null}
    </div>
  );
}
