import type { AutomationMode } from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";
import { automationSkill } from "./model";
import { automationRequestError } from "./automationErrors";

/** Suggestions/Add are local drafts; only sending resolves the shared command. */
export async function prepareAutomationMessage(text: string, mode: AutomationMode | undefined, nodeId?: string) {
  const command = text.trimStart().startsWith("/")
    ? (await getTalosClient().automation.resolveCommand({ text })).command : null;
  if (command && command.type !== "activate") {
    if (mode) throw automationRequestError("selectedModeManagementConflict");
    return { kind: "management" as const, command };
  }
  if (mode && command?.type === "activate" && mode !== command.mode) {
    throw automationRequestError("selectedModeConflict");
  }
  const selectedMode = command?.type === "activate" ? command.mode : mode;
  if (selectedMode && !nodeId) throw automationRequestError("computerRequired");
  return { kind: "message" as const, prompt: command?.type === "activate" ? command.prompt : text,
    skillSelection: selectedMode && nodeId ? await automationSkill(selectedMode, nodeId) : undefined };
}
