import { vi } from "vitest";
import type { AppProtocol } from "@talos/sdk-browser";
import type { SessionAutomationController } from "./useSessionAutomation";
export const loop = (extra: Partial<AppProtocol.SessionLoop> = {}): AppProtocol.SessionLoop => ({
  id: "loop-1", sessionId: "s1", prompt: "Check CI", schedule: { type: "modelDecided" }, configRevision: 3,
  status: "active", stopReason: null, reason: null, nextDueAt: null,
  latestTurnId: null, createdAt: 1, updatedAt: 2, ...extra,
});
export const goal = (extra: Partial<AppProtocol.SessionGoal> = {}): AppProtocol.SessionGoal => ({
  id: "goal-1", sessionId: "s1", objective: "Fix CI", objectiveRevision: 2, status: "active", pauseReason: null,
  reason: null, tokenBudget: 2000, tokensUsed: 100, latestTurnId: null, completionEvidence: null,
  createdAt: 1, updatedAt: 2, ...extra,
});
export function automationController(extra: Partial<SessionAutomationController> = {}): SessionAutomationController {
  return {
    tasks: [], commands: [{ mode: "goal", command: "/goal", description: "Goal" }, { mode: "loop", command: "/loop", description: "Loop" }],
    execute: vi.fn().mockResolvedValue({ effect: "applied", createdObject: null, boundOrigin: null, snapshot: { sessionId: "s1", goal: null, loops: [] } }),
    refresh: vi.fn().mockResolvedValue(undefined), command: vi.fn().mockResolvedValue(undefined), editing: undefined,
    setEditing: vi.fn(), pending: false, error: undefined, readRequest: undefined, ...extra,
  };
}
