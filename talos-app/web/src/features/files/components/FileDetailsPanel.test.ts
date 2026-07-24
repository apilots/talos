import { describe, expect, it } from "vitest";

import { parseFileMetadataInput } from "./FileDetailsPanel";

describe("parseFileMetadataInput", () => {
  it("accepts a string map", () => {
    expect(parseFileMetadataInput('{"source":"upload"}')).toEqual({ source: "upload" });
  });

  it("treats an empty editor as an explicit clear", () => {
    expect(parseFileMetadataInput("  ")).toEqual({});
  });

  it("rejects non-string metadata values", () => {
    expect(parseFileMetadataInput('{"page":1}')).toBeNull();
  });
});
