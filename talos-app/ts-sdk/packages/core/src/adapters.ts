import {
  ErrorCode,
  type ApiError,
  type ApiResponse,
  type TimelineEvent,
  type ErrorCode as ErrorCodeType,
} from "./generated/app-protocol";

export type HttpMethod = "GET" | "POST" | "PATCH" | "DELETE" | "PUT";

export interface FileUploadProgress {
  loadedBytes: number;
  totalBytes: number | null;
}

export interface FileUploadOptions {
  signal?: AbortSignal;
  onProgress?: (progress: FileUploadProgress) => void;
}

export interface HttpRequestOptions {
  method?: HttpMethod;
  body?: string | FormData;
  headers?: Record<string, string>;
  signal?: AbortSignal;
  onUploadProgress?: (progress: FileUploadProgress) => void;
}

export interface StreamRequestOptions extends HttpRequestOptions {
  signal?: AbortSignal;
  onOpen?: () => void;
}

export interface HttpAdapter {
  request<T>(url: string, options?: HttpRequestOptions): Promise<T>;
}

export interface StreamAdapter {
  stream(
    url: string,
    options: StreamRequestOptions,
    onMessage: (data: string, event?: string) => void,
    onError?: (error: Error) => void,
  ): Promise<void>;
}

export interface TalosHttpErrorPayload {
  status: number;
  statusText: string;
  body: string;
  url: string;
  method: string;
}

const ERROR_CODE_VALUES = new Set<string>(Object.values(ErrorCode));

export class TalosHttpError extends Error {
  public readonly status: number;
  public readonly statusText: string;
  public readonly body: string;
  public readonly url: string;
  public readonly method: string;

  constructor(payload: TalosHttpErrorPayload) {
    super(`Request failed with ${payload.status} ${payload.statusText}`);
    this.name = "TalosHttpError";
    this.status = payload.status;
    this.statusText = payload.statusText;
    this.body = payload.body;
    this.url = payload.url;
    this.method = payload.method;
  }
}

/** Carry a typed failure; untrusted transport payloads must pass decodeApiError first. */
export class TalosApiError extends Error {
  public readonly code: ErrorCodeType;
  public readonly apiError: ApiError;

  constructor(error: ApiError, options?: ErrorOptions) {
    super(error.message, options);
    this.name = "TalosApiError";
    this.code = error.code;
    this.apiError = error;
  }
}

/** Decode transport JSON without exposing parser excerpts in the public failure. */
export function parseTalosJson(text: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch (cause) {
    throw new TalosApiError({
      code: ErrorCode.InternalError,
      message: "The backend returned invalid JSON.",
      localization: null,
    }, { cause });
  }
}

export function parseTalosApiErrorBody(body: string): ApiError | null {
  let value: unknown;
  try {
    value = JSON.parse(body) as unknown;
  } catch {
    return null;
  }
  if (!value || typeof value !== "object") {
    return null;
  }
  const candidate = value as Record<string, unknown>;
  if (candidate.status !== "error") {
    return null;
  }
  return decodeApiError(candidate.error);
}

export function unwrapTalosApiResponse<T>(value: unknown): T {
  if (!value || typeof value !== "object") {
    throw new Error("Invalid Talos API response.");
  }
  const candidate = value as Record<string, unknown>;
  const response = candidate as ApiResponse<T>;
  if (response.status === "success") {
    return response.data;
  }
  if (response.status === "error") {
    throw new TalosApiError(decodeApiError(response.error));
  }
  throw new Error("Invalid Talos API response.");
}

/** Validate the shared public failure at transport and event ingress. */
export function decodeApiError(value: unknown): ApiError {
  if (!value || typeof value !== "object") {
    throw new Error("Invalid Talos API response.");
  }
  const error = value as Record<string, unknown>;
  if (
    typeof error.code !== "string" ||
    !ERROR_CODE_VALUES.has(error.code) ||
    typeof error.message !== "string" ||
    error.message.trim().length === 0 ||
    [...error.message].length > 4096 ||
    !("localization" in error) ||
    Object.keys(error).some((key) => !["code", "message", "localization"].includes(key))
  ) {
    throw new Error("Invalid Talos API response.");
  }
  let localization: ApiError["localization"] = null;
  if (error.localization !== null) {
    if (!error.localization || typeof error.localization !== "object") {
      throw new Error("Invalid Talos API response.");
    }
    const descriptor = error.localization as Record<string, unknown>;
    switch (descriptor.type) {
      case "modelNotConfigured": {
        const { provider, model } = descriptor;
        if (typeof provider !== "string" || typeof model !== "string" ||
            !provider.trim() || !model.trim() || [...provider].length > 256 || [...model].length > 256 ||
            error.code !== ErrorCode.ProviderUnavailable) {
          throw new Error("Invalid Talos API response.");
        }
        localization = { type: "modelNotConfigured", provider, model };
        break;
      }
      case "attachmentCountExceeded": {
        const { limit, actual } = descriptor;
        if (typeof limit !== "number" || typeof actual !== "number" ||
            !Number.isSafeInteger(limit) || !Number.isSafeInteger(actual) || limit < 0 || actual <= limit ||
            error.code !== ErrorCode.AttachmentLimitExceeded) {
          throw new Error("Invalid Talos API response.");
        }
        localization = { type: "attachmentCountExceeded", limit, actual };
        break;
      }
      default:
        throw new Error("Invalid Talos API response.");
    }
  }
  return {
    code: error.code as ErrorCodeType,
    message: error.message,
    localization,
  };
}

/** Validate terminal errors identically for live transport and replay consumers. */
export function parseTimelineEvent(value: unknown): TimelineEvent {
  const record = value as TimelineEvent | null;
  if (!record?.event?.data?.identity || !["agent", "systemNotice"].includes(record.event.type)) {
    throw new Error("Invalid Talos timeline event.");
  }
  if (record.event.type === "systemNotice") {
    validateSystemNotice(record.event.data.notice);
    return record;
  }
  const payload = record.event.data.payload;
  if (!payload || typeof payload.type !== "string") throw new Error("Invalid Talos Agent event.");
  if (payload.type === "turnFailed") {
    const data = payload.data;
    if (!data || typeof data.retryable !== "boolean") {
      throw new Error("Invalid Talos terminal failure.");
    }
    decodeApiError(data.error);
  }
  if (payload.type === "messageBlockStarted" || payload.type === "messageBlockCompleted") {
    if (payload.data.block.content.type === "systemNotice") validateSystemNotice(payload.data.block.content.data);
  }
  if (payload.type === "messageStarted" || payload.type === "messageCompleted") {
    const message = payload.type === "messageStarted" ? payload.data.message : payload.data;
    for (const block of message.blocks) {
      if (block.content.type === "systemNotice") validateSystemNotice(block.content.data);
    }
  }
  return record;
}

function validateSystemNotice(notice: import("./generated/app-protocol").AgentSystemNotice): void {
  if (!notice || !["info", "warning", "error"].includes(notice.level)) throw new Error("Invalid Talos notice level.");
  if (notice.content?.type === "error") {
    decodeApiError(notice.content.data);
  } else if (notice.content?.type === "promptWarning") {
    const data = notice.content.data;
    const warning = data?.warning;
    const validCount = (value: number) => Number.isInteger(value) && value >= 0 && value <= 0xffff_ffff;
    const validFile = warning && "file" in warning && ["agents", "soul", "identity", "user", "heartbeat"].includes(warning.file);
    const validWarning = warning && (
      ((warning.type === "bootstrapFileMissing" || warning.type === "bootstrapFileDropped") && validFile)
      || (warning.type === "bootstrapFileTruncated" && validFile && validCount(warning.injectedChars) && validCount(warning.rawChars))
      || (warning.type === "dynamicSectionsDropped" && validCount(warning.count))
    );
    if (typeof data?.message !== "string" || !validWarning) throw new Error("Invalid Talos prompt warning.");
  } else if (notice.content?.type !== "text" || typeof notice.content.data !== "string") {
    throw new Error("Invalid Talos notice content.");
  }
}
