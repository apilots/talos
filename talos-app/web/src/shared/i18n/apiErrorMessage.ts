import { AppProtocol, TalosApiError } from "@talos/sdk-browser";

import { i18n } from "./setup";

export function apiErrorMessage(error: AppProtocol.ApiError | TalosApiError): string {
  const apiError = error instanceof TalosApiError ? error.apiError : error;
  return i18n.t(`errors:${apiError.code}`, {
    defaultValue: apiError.message,
  });
}

export function userVisibleErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof TalosApiError) {
    return apiErrorMessage(error);
  }
  return error instanceof Error ? error.message : fallback;
}
