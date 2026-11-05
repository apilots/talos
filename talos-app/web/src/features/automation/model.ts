import type { AppProtocol } from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";
import { automationRequestError } from "./automationErrors";

export type AutomationTask = { type: "goal"; value: AppProtocol.SessionGoal } | { type: "loop"; value: AppProtocol.SessionLoop };
export type AutomationMode = AppProtocol.AutomationMode;
export interface AutomationTurnTiming {
  turnId: string;
  origin: AppProtocol.AutomationTurnOrigin;
  startedAt: number;
}

export function snapshotTasks(snapshot: AppProtocol.SessionAutomationSnapshot | undefined): AutomationTask[] {
  const goal = snapshot?.goal;
  const loops = (snapshot?.loops ?? []).filter((value) => value.status === "active");
  return [...(goal && (goal.status === "active" || goal.status === "paused") ? [{ type: "goal" as const, value: goal }] : []),
    ...loops.sort((left, right) => left.createdAt - right.createdAt || left.id.localeCompare(right.id)).map((value) => ({ type: "loop" as const, value }))];
}
export function taskPrompt(task: AutomationTask): string {
  return task.type === "goal" ? task.value.objective : task.value.prompt;
}
export function taskCanEdit(task: AutomationTask): boolean {
  return task.type === "goal" ? task.value.status === "active" || task.value.status === "paused" : task.value.status === "active";
}

/** Resolve the protected read-only System Skill on the Session's actual Worker. */
export async function automationSkill(mode: AutomationMode, nodeId: string) {
  const { data } = await getTalosClient(nodeId).skills.list({ nodeId, include_disabled: false });
  const skill = data.find((entry) => entry.name === mode && entry.scope === "system" && entry.enabled && entry.read_only);
  if (!skill) throw automationRequestError("systemSkillUnavailable");
  return { name: skill.name, path: skill.path };
}
