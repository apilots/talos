import { describe, expect, it } from "vitest";

import {
  parseTalosApiErrorBody,
  TalosApiError,
  TalosHttpError,
  unwrapTalosApiResponse,
} from "../adapters";
import { ErrorCode } from "../generated/app-protocol";

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
          code: ErrorCode.PathNotFound,
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
          code: ErrorCode.ActiveTurnRunning,
          message: "Active turn is already running",
        },
      }),
    ).toThrow(TalosApiError);

    try {
      unwrapTalosApiResponse({
        status: "error",
        error: {
          code: ErrorCode.ActiveTurnRunning,
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
