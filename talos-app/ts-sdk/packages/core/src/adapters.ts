import {
  ErrorCode,
  type ApiError,
  type ApiResponse,
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

export class TalosApiError extends Error {
  public readonly code: ErrorCodeType;
  public readonly apiError: ApiError;

  constructor(error: ApiError) {
    super(error.message);
    this.name = "TalosApiError";
    this.code = error.code;
    this.apiError = error;
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

function decodeApiError(value: unknown): ApiError {
  if (!value || typeof value !== "object") {
    throw new Error("Invalid Talos API response.");
  }
  const error = value as Record<string, unknown>;
  if (
    typeof error.code !== "string" ||
    !ERROR_CODE_VALUES.has(error.code) ||
    typeof error.message !== "string"
  ) {
    throw new Error("Invalid Talos API response.");
  }
  return {
    code: error.code as ErrorCodeType,
    message: error.message,
  };
}
