import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import type { AppProtocol } from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";
import { useProductScope } from "@/features/computers";
import { apiErrorMessage, userVisibleError, userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { snapshotTasks, type AutomationTask } from "./model";
import { automationRequestError } from "./automationErrors";

export function useSessionAutomation(sessionId: string, enabled: boolean) {
  const scope = useProductScope();
  const client = useQueryClient();
  const owner = JSON.stringify([scope, sessionId]);
  const ownerRef = useRef(owner);
  ownerRef.current = owner;
  const [uiOwner, setUiOwner] = useState(owner);
  const [editing, saveEditing] = useState<AutomationTask>();
  const [error, setError] = useState<ReturnType<typeof userVisibleError>>();
  const [pending, setPending] = useState(false);
  const [readRequest, setReadRequest] = useState<{ mode: AppProtocol.AutomationMode; serial: number }>();
  useEffect(() => { setUiOwner(owner); saveEditing(undefined); setError(undefined); setPending(false); setReadRequest(undefined); }, [owner]);
  const setEditing = (task: AutomationTask | undefined) => { if (ownerRef.current === owner) saveEditing(task); };
  const query = useQuery({ queryKey: ["talos", scope, "automation", sessionId],
    queryFn: () => getTalosClient().automation.read({ sessionId }), enabled,
    refetchInterval: 5_000, retry: false });
  const catalog = useQuery({ queryKey: ["talos", scope, "automation-commands"],
    queryFn: () => getTalosClient().automation.commandCatalog({}), enabled, staleTime: Infinity });
  const refresh = async () => {
    if (ownerRef.current !== owner) return;
    await client.invalidateQueries({ queryKey: ["talos", scope],
      predicate: (entry) => entry.queryKey[2] === "automation" || entry.queryKey.includes(sessionId) });
  };
  const execute = async (action: AppProtocol.AutomationAction) => {
    setPending(true); setError(undefined);
    try {
      const result = await getTalosClient().automation.execute({ sessionId, action });
      if (ownerRef.current !== owner) return result;
      client.setQueryData(["talos", scope, "automation", sessionId], result.snapshot);
      await refresh();
      return result;
    } catch (failure) {
      if (ownerRef.current === owner) setError(userVisibleError(failure, "Automation operation failed."));
      throw failure;
    } finally { if (ownerRef.current === owner) setPending(false); }
  };
  const command = async (value: AppProtocol.AutomationCommand): Promise<void> => {
    const snapshot = value.type === "activate" ? query.data : await getTalosClient().automation.read({ sessionId });
    if (ownerRef.current !== owner) return;
    const goal = snapshot?.goal;
    switch (value.type) {
      case "read": await refresh(); if (ownerRef.current === owner) setReadRequest((current) => ({ mode: value.mode, serial: (current?.serial ?? 0) + 1 })); return;
      case "edit": {
        const task = snapshotTasks(snapshot).find((entry) => entry.type === value.mode && (!value.objectId || entry.value.id === value.objectId));
        if (!task) throw automationRequestError("taskNotFound");
        setEditing(task); return;
      }
      case "pauseGoal": if (!goal) throw automationRequestError("goalNotFound"); await execute({ target: "goal", action: { type: "pause", goalId: goal.id } }); return;
      case "resumeGoal": if (!goal) throw automationRequestError("goalNotFound"); await execute({ target: "goal", action: { type: "resume", goalId: goal.id } }); return;
      case "clearGoal": if (!goal) throw automationRequestError("goalNotFound"); await execute({ target: "goal", action: { type: "clear", goalId: goal.id } }); return;
      case "stopLoop": await execute({ target: "loop", action: { type: "stop", loopId: value.loopId } }); return;
      case "clearLoop": await execute({ target: "loop", action: { type: "clear", loopId: value.loopId } }); return;
      case "activate": return;
    }
  };
  const readError = query.error;
  return { tasks: snapshotTasks(query.data), commands: catalog.data?.data ?? [], execute, refresh, command,
    editing: uiOwner === owner ? editing : undefined, setEditing, pending: uiOwner === owner && pending, readRequest: uiOwner === owner ? readRequest : undefined, error: uiOwner === owner && error ? apiErrorMessage(error) : (readError ? userVisibleErrorMessage(readError, "Automation could not be loaded.") : undefined) };
}
export type SessionAutomationController = ReturnType<typeof useSessionAutomation> & {
  activeTurn?: import("./model").AutomationTurnTiming;
};
