import { useMutation } from "@tanstack/react-query";

import { submitToolApproval } from "../api";
import { useAppStore } from "@/shared/store/app-store";

export function useSessionApproval(
  sessionId: string,
  continueAfterApproval: (text: string) => Promise<unknown>,
) {
  const pendingApproval = useAppStore((state) => state.streams[sessionId]?.pendingApproval);

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
      useAppStore.getState().setSessionStatus(sessionId, "running");
      const continuation = approved
        ? ""
        : `[Tool \`${pendingApproval.toolName}\` was rejected by user.]`;
      await continueAfterApproval(continuation);
    },
  });

  return {
    pendingApproval,
    submitApproval: (approved: boolean) => approvalMutation.mutateAsync(approved),
  };
}
