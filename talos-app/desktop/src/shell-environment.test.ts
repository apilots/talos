import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";

import { afterEach } from "vitest";
import { describe, expect, it } from "vitest";

import {
  parseEnvironment,
  readConfigPlaceholderNames,
  resolveAppServerEnvironment,
} from "./shell-environment";

const tempDirs: string[] = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("parseEnvironment", () => {
  it("parses shell environment output", () => {
    expect(parseEnvironment("A=1\nB=value=with=equals\n\n")).toEqual({
      A: "1",
      B: "value=with=equals",
    });
  });
});

describe("resolveAppServerEnvironment", () => {
  it("merges login shell environment for packaged macOS runtime", async () => {
    const env = await resolveAppServerEnvironment({
      baseEnv: {
        EXISTING: "from-process",
        SHELL_ONLY: "process-wins",
      },
      isPackaged: true,
      platform: "darwin",
      execLoginShell: async () => ({
        SHELL_ONLY: "from-shell",
        BAILIAN_CODINGPLAN_API_KEY: "from-shell",
      }),
    });

    expect(env).toMatchObject({
      EXISTING: "from-process",
      SHELL_ONLY: "process-wins",
      BAILIAN_CODINGPLAN_API_KEY: "from-shell",
    });
  });

  it("does not read login shell environment outside packaged macOS runtime", async () => {
    const env = await resolveAppServerEnvironment({
      baseEnv: { EXISTING: "from-process" },
      isPackaged: false,
      platform: "darwin",
      execLoginShell: async () => {
        throw new Error("should not be called");
      },
    });

    expect(env).toEqual({ EXISTING: "from-process" });
  });
});

describe("readConfigPlaceholderNames", () => {
  it("extracts unique environment placeholder names from config", () => {
    const dir = mkdtempSync(path.join(os.tmpdir(), "talos-desktop-env-test-"));
    tempDirs.push(dir);
    const configPath = path.join(dir, "talos.yaml");
    writeFileSync(
      configPath,
      [
        "gateway:",
        "  jwt_secret: ${JWT_SECRET}",
        "llm:",
        "  anthropic:",
        "    api_key: ${BAILIAN_CODINGPLAN_API_KEY}",
        "  openai:",
        "    api_key: ${BAILIAN_CODINGPLAN_API_KEY}",
      ].join("\n"),
    );

    expect(readConfigPlaceholderNames(configPath)).toEqual([
      "BAILIAN_CODINGPLAN_API_KEY",
      "JWT_SECRET",
    ]);
  });
});
