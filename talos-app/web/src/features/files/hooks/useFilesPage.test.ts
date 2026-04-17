import { describe, expect, it } from "vitest";

import { classifyFile, getFileExtension } from "./useFilesPage";

describe("useFilesPage helpers", () => {
  it("extracts lowercase file extensions", () => {
    expect(getFileExtension("src/App.TSX")).toBe("tsx");
  });

  it("classifies common file types", () => {
    expect(
      classifyFile({
        id: "1",
        label: "main.rs",
        path: "src/main.rs",
        summary: "Rust entry",
        updatedAt: Date.now(),
      }),
    ).toBe("code");
    expect(
      classifyFile({
        id: "2",
        label: "report.csv",
        path: "exports/report.csv",
        summary: "CSV export",
        updatedAt: Date.now(),
      }),
    ).toBe("data");
  });
});
