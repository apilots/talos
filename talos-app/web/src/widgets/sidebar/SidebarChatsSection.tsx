import { useState } from "react";
import { useTranslation } from "react-i18next";

import { SidebarSectionHeader } from "./SidebarSectionHeader";
import { SidebarSessionRow, type SidebarSessionItem } from "./SidebarSessionRow";

interface SidebarChatsSectionProps {
  currentPath: string;
  sessions: SidebarSessionItem[];
  onSelectSession: (sessionId: string) => void;
  onDeleteSession: (session: SidebarSessionItem) => void;
  onTogglePinSession?: (sessionId: string, pinned: boolean) => Promise<void> | void;
  pendingPinSessionIds?: ReadonlySet<string>;
}

export function SidebarChatsSection({
  currentPath,
  sessions,
  onSelectSession,
  onDeleteSession,
  onTogglePinSession,
  pendingPinSessionIds,
}: SidebarChatsSectionProps) {
  const { t } = useTranslation(["sidebar", "common"]);
  const [open, setOpen] = useState(true);
  const [expanded, setExpanded] = useState(false);
  const visibleSessions = expanded ? sessions : sessions.slice(0, 10);

  return (
    <section className="mt-6" data-sidebar-section="chats">
      <SidebarSectionHeader label={t("threads")} onToggle={() => setOpen((value) => !value)} open={open} />
      {open ? (
        <div className="mt-1 space-y-1">
          {visibleSessions.map((session) => (
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
          {sessions.length > 10 && !expanded ? (
            <button
              className="px-3 py-2 text-left text-sm text-muted transition hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
              onClick={() => setExpanded(true)}
              type="button"
            >
              {t("common:showMore")}
            </button>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
