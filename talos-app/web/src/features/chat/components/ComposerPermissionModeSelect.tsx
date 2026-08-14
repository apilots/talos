import { Shield, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { UserPermissionMode } from "@talos/sdk-browser";

import { ComposerDropdown } from "./ComposerDropdown";

interface ComposerPermissionModeSelectProps {
  disabled: boolean;
  onChange?: (mode: UserPermissionMode) => void;
  value: UserPermissionMode;
}

export function ComposerPermissionModeSelect({
  disabled,
  onChange,
  value,
}: ComposerPermissionModeSelectProps) {
  const { t } = useTranslation("chat");
  return (
    <ComposerDropdown
      disabled={disabled}
      header={t("agentAccessMode")}
      items={[
        {
          value: "askForApproval",
          label: t("askForApproval"),
          description: t("askForApprovalDescription"),
          icon: <Shield size={13} />,
        },
        {
          value: "approveForMe",
          label: t("approveForMe"),
          description: t("approveForMeDescription"),
          icon: <Shield size={13} />,
        },
        {
          value: "fullAccess",
          label: t("fullAccess"),
          description: t("fullAccessDescription"),
          icon: <TriangleAlert size={13} />,
        },
      ]}
      onChange={(mode) => onChange?.(mode as UserPermissionMode)}
      value={value}
    />
  );
}
