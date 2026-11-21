import { AppProtocol, TalosApiError } from "@talos/sdk-browser";

export function isServiceUnavailable(error: unknown) {
  return error instanceof TypeError || (error instanceof TalosApiError &&
    (error.code === AppProtocol.ErrorCode.ServiceUnavailable || error.code === AppProtocol.ErrorCode.Timeout));
}
