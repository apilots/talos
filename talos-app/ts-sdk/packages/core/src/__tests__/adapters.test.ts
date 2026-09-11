import { describe, expect, it } from "vitest";

import {
  decodeApiError,
  parseTimelineEvent,
  parseTalosApiErrorBody,
  TalosApiError,
  TalosHttpError,
  unwrapTalosApiResponse,
} from "../adapters";
import { ErrorCode } from "../generated/app-protocol";

describe("shared error contract", () => {
  const valid = {
    code: ErrorCode.AttachmentLimitExceeded,
    message: "This request contains 6 attachments; the limit is 4.",
    localization: { type: "attachmentCountExceeded", limit: 4, actual: 6 },
  } as const;
  it("preserves exact parameterized failure through serialization", () => {
    for (const message of [valid.message, "The request has too many attachments: 6 supplied, 4 allowed."]) {
      const error = { ...valid, message };
      expect(decodeApiError(JSON.parse(JSON.stringify(error)))).toEqual(error);
    }
  });

  it.each([
    { ...valid, code: "futureCode" },
    { ...valid, code: ErrorCode.InternalError },
    { ...valid, message: "" },
    { code: valid.code, message: valid.message },
    { ...valid, localization: { ...valid.localization, type: "futureType" } },
    { ...valid, localization: { ...valid.localization, actual: Number.MAX_SAFE_INTEGER + 1 } },
    { ...valid, localization: { ...valid.localization, actual: valid.localization.limit } },
    { ...valid, localization: { ...valid.localization, limit: -1 } },
    { ...valid, message: "x".repeat(4097) },
  ])("rejects invalid payload instead of erasing its cause", (error) => {
    expect(() => decodeApiError(error)).toThrow("Invalid Talos API response.");
  });

  it("bounds Unicode scalars rather than UTF-16 code units", () => {
    const error = { code: ErrorCode.InternalError, message: "😀".repeat(4096), localization: null };
    expect(decodeApiError(error)).toEqual(error);
  });
});

describe("TalosHttpError", () => {
  it("represents non-application HTTP failures", () => {
    const error = new TalosHttpError({
      method: "GET",
      url: "http://localhost:8080/v1/fs/directory?path=%2Fmissing",
      status: 404,
      statusText: "Not Found",
      body: JSON.stringify({
        status: "error",
        error: {
          code: ErrorCode.PathNotFound, localization: null,
          message: "Path not found",
        },
      }),
    });

    expect(error.status).toBe(404);
    expect(error.message).toBe("Request failed with 404 Not Found");
  });

  it("falls back to status text for non-structured errors", () => {
    const error = new TalosHttpError({
      method: "GET",
      url: "http://localhost:8080/v1/fs/directory",
      status: 500,
      statusText: "Internal Server Error",
      body: "proxy failure",
    });

    expect(error.message).toBe("Request failed with 500 Internal Server Error");
  });
});

describe("parseTalosApiErrorBody", () => {
  it("returns null for malformed error bodies", () => {
    expect(parseTalosApiErrorBody("{")).toBeNull();
    expect(parseTalosApiErrorBody(JSON.stringify({ code: ErrorCode.PathNotFound }))).toBeNull();
  });

  it("rejects malformed structured error bodies", () => {
    expect(() => parseTalosApiErrorBody(JSON.stringify({ status: "error" }))).toThrow(
      "Invalid Talos API response.",
    );
    expect(() =>
      parseTalosApiErrorBody(
        JSON.stringify({
          status: "error",
          error: { code: "futureCode", message: "Future error" },
        }),
      ),
    ).toThrow("Invalid Talos API response.");
  });
});

describe("unwrapTalosApiResponse", () => {
  it("returns success data", () => {
    expect(
      unwrapTalosApiResponse<{ sessionId: string }>({
        status: "success",
        data: { sessionId: "session-1" },
      }),
    ).toEqual({ sessionId: "session-1" });
  });

  it("preserves structured application errors", () => {
    expect(() =>
      unwrapTalosApiResponse({
        status: "error",
        error: {
          code: ErrorCode.ActiveTurnRunning, localization: null,
          message: "Active turn is already running",
        },
      }),
    ).toThrow(TalosApiError);

    try {
      unwrapTalosApiResponse({
        status: "error",
        error: {
          code: ErrorCode.ActiveTurnRunning, localization: null,
          message: "Active turn is already running",
        },
      });
    } catch (error) {
      expect(error).toMatchObject({
        name: "TalosApiError",
        code: ErrorCode.ActiveTurnRunning,
        message: "Active turn is already running",
      } satisfies Partial<TalosApiError>);
    }
  });

  it("rejects malformed API responses", () => {
    expect(() => unwrapTalosApiResponse({ data: {} })).toThrow("Invalid Talos API response.");
    expect(() =>
      unwrapTalosApiResponse({
        status: "error",
        error: { code: "futureCode", message: "Future error" },
      }),
    ).toThrow("Invalid Talos API response.");
  });
});


describe("prompt warning ingress", () => {
  const notice = { level: "warning", content: { type: "promptWarning", data: {
    message: "Prompt bootstrap file was truncated: IDENTITY.md (10 of 20 characters)",
    warning: { type: "bootstrapFileTruncated", file: "identity", injectedChars: 10, rawChars: 20 },
  } } };
  const event = (value: unknown) => ({ event: { type: "agent", data: {
    identity: { sessionId: "s", turnId: "t" }, payload: { type: "messageBlockCompleted", data: {
      block: { content: { type: "systemNotice", data: value } },
    } },
  } } });
  it("retains typed prompt warning parameters for live and replay presentation", () => {
    const record = event(notice);
    expect(parseTimelineEvent(JSON.parse(JSON.stringify(record)))).toEqual(record);
  });
  it.each([
    { ...notice.content.data.warning, file: "unknown" },
    { ...notice.content.data.warning, injectedChars: -1 },
    { ...notice.content.data.warning, rawChars: 2 ** 32 },
    { ...notice.content.data.warning, type: "unknown" },
  ])("rejects invalid warning parameters", (warning) => {
    const invalid = { ...notice, content: { ...notice.content, data: { ...notice.content.data, warning } } };
    expect(() => parseTimelineEvent(event(invalid))).toThrow("Invalid Talos prompt warning.");
  });
});
