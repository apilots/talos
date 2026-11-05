import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { automationResources } from "@/shared/i18n/automationResources";

/** Local guards use the same structured, render-time localization as API failures. */
export function automationRequestError(reason: AppProtocol.AutomationRequestReason): TalosApiError {
  const codes: Record<AppProtocol.AutomationRequestReason, AppProtocol.ErrorCode> = {
    selectedModeManagementConflict: AppProtocol.ErrorCode.Conflict,
    selectedModeConflict: AppProtocol.ErrorCode.Conflict,
    computerRequired: AppProtocol.ErrorCode.InvalidRequest,
    chatRequired: AppProtocol.ErrorCode.InvalidRequest,
    taskNotFound: AppProtocol.ErrorCode.ResourceNotFound,
    goalNotFound: AppProtocol.ErrorCode.ResourceNotFound,
    systemSkillUnavailable: AppProtocol.ErrorCode.ResourceNotFound,
  };
  return new TalosApiError({
    code: codes[reason],
    message: automationResources.en.requestErrors[reason],
    localization: { type: "automationRequest", reason },
  });
}
