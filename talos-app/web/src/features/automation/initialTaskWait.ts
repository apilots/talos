import type { AutomationTask, AutomationTurnTiming } from "./model";

/** Mirrors the Service's initial grace period; this only renders existing facts. */
const INITIAL_START_DELAY_SECONDS = 10;

export function initialTaskWait(task: AutomationTask, activeTurn?: AutomationTurnTiming, queuedLoop = false): { dueAt: number | null } | undefined {
  if (task.value.status !== "active" || task.value.latestTurnId !== null) return;
  if (task.type === "goal") {
    return { dueAt: task.value.createdAt + INITIAL_START_DELAY_SECONDS };
  }
  if (activeTurn?.origin.type === "loop" && activeTurn.origin.loopId === task.value.id) return;
  if (task.value.nextDueAt !== null || queuedLoop) return { dueAt: task.value.nextDueAt };
}
