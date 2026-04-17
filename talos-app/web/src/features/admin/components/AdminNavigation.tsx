import type { LucideIcon } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { AdminSection } from "../hooks/useAdminPage";
import { Panel } from "@/shared/ui/Panel";

export function AdminNavigation({
  activeSection,
  sectionIcons,
  sectionLabels,
  sections,
  setActiveSection,
}: {
  activeSection: AdminSection;
  sectionIcons: Record<AdminSection, LucideIcon>;
  sectionLabels: Record<AdminSection, string>;
  sections: AdminSection[];
  setActiveSection: (section: AdminSection) => void;
}) {
  const { t } = useTranslation("admin");

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-3">
      <div className="mb-3 px-3 pt-2 text-xs uppercase tracking-[0.18em] text-muted">
        {t("shellLabel")}
      </div>
      <nav className="space-y-1">
        {sections.map((section) => {
          const Icon = sectionIcons[section];
          return (
            <button
              className={[
                "flex w-full items-center gap-3 rounded-2xl px-4 py-3 text-left text-sm transition",
                activeSection === section
                  ? "bg-background/70 text-ink shadow-panel"
                  : "text-muted hover:bg-background/45 hover:text-ink",
              ].join(" ")}
              key={section}
              onClick={() => setActiveSection(section)}
              type="button"
            >
              <Icon size={16} />
              <span>{sectionLabels[section]}</span>
            </button>
          );
        })}
      </nav>
    </Panel>
  );
}
