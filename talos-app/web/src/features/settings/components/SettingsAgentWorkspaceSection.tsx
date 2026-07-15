import { Brain } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { useConnectionSettings } from "@/features/connection";
import { Panel } from "@/shared/ui/Panel";
import { AgentWorkspaceGeneralPanel } from "./agent-workspace/AgentWorkspaceGeneralPanel";
import { AgentWorkspaceInstructionsPanel } from "./agent-workspace/AgentWorkspaceInstructionsPanel";
import { AgentWorkspaceMemoryPanel } from "./agent-workspace/AgentWorkspaceMemoryPanel";
import { SectionHeader } from "./SectionPrimitives";

type AgentWorkspaceMode = "general" | "instructions" | "memory";

const workspaceModes: AgentWorkspaceMode[] = ["general", "instructions", "memory"];

export function SettingsAgentWorkspaceSection() {
  const { t } = useTranslation("settings");
  const { connection } = useConnectionSettings();
  const canEdit = connection.authStatus === "open" || connection.authStatus === "authenticated";
  const [mode, setMode] = useState<AgentWorkspaceMode>("general");

  return (
    <div className="space-y-5">
      <Panel className="rounded-[var(--radius-panel)] border-border/70 bg-background/35 p-6 shadow-none backdrop-blur-none">
        <SectionHeader
          icon={<Brain size={18} />}
          subtitle={t("agentWorkspaceDetail")}
          title={t("agentWorkspaceTitle")}
        />
        <div className="mt-5 flex gap-1 rounded-[var(--radius-control)] border border-border/70 bg-surface/50 p-1">
          {workspaceModes.map((workspaceMode) => (
            <button
              aria-pressed={mode === workspaceMode}
              className={[
                "flex min-w-0 flex-1 items-center justify-center rounded-[calc(var(--radius-control)-0.2rem)] px-3 py-2 text-sm font-medium transition",
                mode === workspaceMode ? "bg-card text-ink shadow-sm" : "text-muted hover:text-ink",
              ].join(" ")}
              key={workspaceMode}
              onClick={() => setMode(workspaceMode)}
              type="button"
            >
              {modeLabel(workspaceMode, t)}
            </button>
          ))}
        </div>
      </Panel>

      {mode === "general" ? <AgentWorkspaceGeneralPanel /> : null}
      {mode === "instructions" ? <AgentWorkspaceInstructionsPanel canEdit={canEdit} /> : null}
      {mode === "memory" ? <AgentWorkspaceMemoryPanel canEdit={canEdit} /> : null}
    </div>
  );
}

function modeLabel(
  mode: AgentWorkspaceMode,
  t: ReturnType<typeof useTranslation<"settings">>["t"],
) {
  if (mode === "general") {
    return t("agentGeneral");
  }
  if (mode === "instructions") {
    return t("agentInstructions");
  }
  return t("agentMemory");
}
