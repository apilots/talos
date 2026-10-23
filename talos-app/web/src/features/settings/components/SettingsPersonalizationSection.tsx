import { selectionClasses } from "@/shared/ui/controlStyles";
import { Brain } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useConnectionSettings } from "@/features/connection";
import { Panel } from "@/shared/ui/Panel";
import { AgentWorkspaceInstructionsPanel } from "./agent-workspace/AgentWorkspaceInstructionsPanel";
import { AgentWorkspaceMemoryPanel } from "./agent-workspace/AgentWorkspaceMemoryPanel";
import { AgentWorkspaceGeneralPanel } from "./agent-workspace/AgentWorkspaceGeneralPanel";
import { SectionHeader } from "./SectionPrimitives";

type PersonalizationMode = "workspace" | "instructions" | "memory";

const personalizationModes: PersonalizationMode[] = ["workspace", "instructions", "memory"];

export function SettingsPersonalizationSection() {
  const { t } = useTranslation("settings");
  const { connection } = useConnectionSettings();
  const canEdit = connection.authStatus === "open" || connection.authStatus === "authenticated";
  const [mode, setMode] = useState<PersonalizationMode>("workspace");

  return (
    <div className="space-y-5">
      <Panel className="rounded-[var(--radius-panel)] border-border/70 bg-background/35 p-6 shadow-none backdrop-blur-none">
        <SectionHeader
          icon={<Brain size={18} />}
          subtitle={t("personalizationDetail")}
          title={t("personalizationTitle")}
        />
        <div className="mt-5 flex gap-1 rounded-[var(--radius-control)] border border-border/70 bg-surface/50 p-1">
          {personalizationModes.map((personalizationMode) => (
            <button
              aria-pressed={mode === personalizationMode}
              className={[
                "flex min-w-0 flex-1 items-center justify-center rounded-[calc(var(--radius-control)-0.2rem)] px-3 py-2 text-sm font-medium transition",
                mode === personalizationMode ? selectionClasses.selected : selectionClasses.idle,
              ].join(" ")}
              key={personalizationMode}
              onClick={() => setMode(personalizationMode)}
              type="button"
            >
              {t({ workspace: "agentWorkspace", instructions: "agentInstructions", memory: "agentMemory" }[personalizationMode])}
            </button>
          ))}
        </div>
      </Panel>

      {mode === "workspace" ? <AgentWorkspaceGeneralPanel /> : null}
      {mode === "instructions" ? <AgentWorkspaceInstructionsPanel canEdit={canEdit} /> : null}
      {mode === "memory" ? <AgentWorkspaceMemoryPanel canEdit={canEdit} /> : null}
    </div>
  );
}
