import { describe, expect, it } from "vitest";

import { parseTalosApiErrorBody, TalosHttpError } from "../adapters";

describe("TalosHttpError", () => {
  it("uses structured API error messages without exposing request URLs", () => {
    const error = new TalosHttpError({
      method: "GET",
      url: "http://localhost:8080/v1/fs/directory?path=%2Fmissing",
      status: 404,
      statusText: "Not Found",
      body: JSON.stringify({
        code: "path_not_found",
        message: "path_not_found: path not found: /missing",
      }),
    });

    expect(error.code).toBe("path_not_found");
    expect(error.responseMessage).toBe("path_not_found: path not found: /missing");
    expect(error.message).toBe("path_not_found: path not found: /missing");
    expect(error.message).not.toContain("localhost");
  });

  it("falls back to status text for non-structured errors", () => {
    const error = new TalosHttpError({
      method: "GET",
      url: "http://localhost:8080/v1/fs/directory",
      status: 500,
      statusText: "Internal Server Error",
      body: "proxy failure",
    });

    expect(error.code).toBeNull();
    expect(error.responseMessage).toBeNull();
    expect(error.message).toBe("Request failed with 500 Internal Server Error");
  });
});

describe("parseTalosApiErrorBody", () => {
  it("returns null for malformed error bodies", () => {
    expect(parseTalosApiErrorBody("{")).toBeNull();
    expect(parseTalosApiErrorBody(JSON.stringify({ code: "path_not_found" }))).toBeNull();
  });
});
