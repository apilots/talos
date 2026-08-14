import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { UserPermissionMode } from "@talos/sdk-browser";

import { useUpdateSessionPermissionModeMutation } from "@/features/chat";

interface UseSessionPermissionModeParams {
  blocked: boolean;
  initialMode?: UserPermissionMode;
  persistedMode?: UserPermissionMode;
  sessionExists: boolean;
  sessionId: string;
}

export function useSessionPermissionMode({
  blocked,
  initialMode = "fullAccess",
  persistedMode,
  sessionExists,
  sessionId,
}: UseSessionPermissionModeParams) {
  const { t } = useTranslation("chat");
  const [draftMode, setDraftMode] = useState<UserPermissionMode>(initialMode);
  const mutation = useUpdateSessionPermissionModeMutation(sessionId);
  const value = persistedMode ?? draftMode;

  const onChange = (mode: UserPermissionMode) => {
    if (mutation.isPending) {
      return;
    }
    if (
      mode === "fullAccess" &&
      value !== "fullAccess" &&
      !window.confirm(t("fullAccessConfirmation"))
    ) {
      return;
    }
    if (!sessionExists) {
      setDraftMode(mode);
      return;
    }
    mutation.mutate(mode);
  };

  return {
    disabled: blocked || mutation.isPending,
    onChange,
    value,
  };
}
