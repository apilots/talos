import { AppProtocol, TalosApiError } from "@talos/sdk-browser";

import { i18n } from "./setup";

export function apiErrorMessage(error: AppProtocol.ApiError | TalosApiError): string {
  const apiError = error instanceof TalosApiError ? error.apiError : error;
  const descriptor = apiError.localization;
  if (!descriptor) return apiError.message;
  switch (descriptor.type) {
    case "computerDeletionActive":
    case "computerRegistrationRemoved":
    case "computerDeletionUnverified":
    case "modelNotConfigured":
    case "attachmentCountExceeded":
      return i18n.t(`errors:detail.${descriptor.type}`, {
        ...descriptor,
        defaultValue: apiError.message,
      });
    default: {
      const exhaustive: never = descriptor;
      throw new Error(`Unsupported error localization: ${String(exhaustive)}`);
    }
  }
}

/** Keep error state language-independent; render it at the component boundary. */
export function userVisibleError(error: unknown, fallback: string): AppProtocol.ApiError {
  if (error instanceof TalosApiError) return error.apiError;
  return { code: AppProtocol.ErrorCode.InternalError, localization: null,
    message: error instanceof Error && error.message.trim() ? error.message : fallback };
}

export function userVisibleErrorMessage(error: unknown, fallback: string): string {
  return apiErrorMessage(userVisibleError(error, fallback));
}
