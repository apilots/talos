import { describe, expect, it } from "vitest";

import { buildToolDisplayTitle } from "./toolDisplay";

describe("buildToolDisplayTitle", () => {
  it.each([
    ["Bash", '{"cmd":"pnpm test"}', "Bash · pnpm test"],
    ["Read", '{"file_path":"src/app.tsx"}', "Read · src/app.tsx"],
    ["Grep", '{"pattern":"assistant_text_committed"}', "Grep · assistant_text_committed"],
    ["WebSearch", '{"query":"Talos timeline"}', "WebSearch · Talos timeline"],
  ])("projects a semantic title for %s", (toolName, argumentsText, expected) => {
    expect(buildToolDisplayTitle(toolName, argumentsText)).toBe(expected);
  });

  it("caps long argument previews at 94 code points", () => {
    const command = "x".repeat(120);
    expect(buildToolDisplayTitle("Bash", JSON.stringify({ cmd: command }))).toBe(
      `Bash · ${"x".repeat(93)}…`,
    );
  });
});
