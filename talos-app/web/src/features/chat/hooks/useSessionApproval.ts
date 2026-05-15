import { useMutation } from "@tanstack/react-query";

import { submitToolApproval } from "../api";
import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";
import { useAppStore } from "@/shared/store/app-store";

export function useSessionApproval(sessionId: string) {
  const pendingApproval = useAppStore((state) => state.streams[sessionId]?.pendingApproval);
  const { invalidateSessionQueries } = useSessionQueryInvalidation(sessionId);

  const approvalMutation = useMutation({
    mutationFn: async (approved: boolean) => {
      if (!pendingApproval) {
        throw new Error("No pending approval.");
      }
      await submitToolApproval(sessionId, {
        call_id: pendingApproval.callId,
        tool_name: pendingApproval.toolName,
        approved,
      });
      useAppStore.getState().setPendingUserAction(sessionId, undefined);
      useAppStore.getState().setPendingApproval(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        awaitingFinalResponse: false,
        showReconnectControls: false,
      });
      useAppStore.getState().setSessionStatus(sessionId, undefined);
      await invalidateSessionQueries();
    },
  });

  return {
    pendingApproval,
    submitApproval: (approved: boolean) => approvalMutation.mutateAsync(approved),
  };
}
