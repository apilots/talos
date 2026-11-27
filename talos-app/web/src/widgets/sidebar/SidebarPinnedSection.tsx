import { useTranslation } from "react-i18next";

import { SidebarSessionRow, type SidebarSessionItem } from "./SidebarSessionRow";

interface SidebarPinnedSectionProps {
  currentPath: string;
  sessions: SidebarSessionItem[];
  onSelectSession: (sessionId: string) => void;
  onDeleteSession: (session: SidebarSessionItem) => void;
  onTogglePinSession?: (sessionId: string, pinned: boolean) => Promise<void> | void;
  pendingPinSessionIds?: ReadonlySet<string>;
}

export function SidebarPinnedSection({
  currentPath,
  sessions,
  onSelectSession,
  onDeleteSession,
  onTogglePinSession,
  pendingPinSessionIds,
}: SidebarPinnedSectionProps) {
  const { t } = useTranslation("sidebar");

  if (sessions.length === 0) {
    return null;
  }

  return (
    <section aria-label={t("pinned")} className="mb-3 mt-1">
      <div className="flex h-8 items-center px-2 text-sm font-medium text-muted">
        <span>{t("pinned")}</span>
      </div>
      <div className="mt-1 space-y-1">
        {sessions.map((session) => (
          <SidebarSessionRow
            currentPath={currentPath}
            key={session.id}
            onDeleteSession={onDeleteSession}
            onSelectSession={onSelectSession}
            onTogglePinSession={onTogglePinSession}
            pinPending={pendingPinSessionIds?.has(session.id) ?? false}
            session={session}
          />
        ))}
      </div>
    </section>
  );
}
