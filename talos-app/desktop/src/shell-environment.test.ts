import { describe, expect, it } from "vitest";

import { filterAppServerEnvironment, resolveAppServerEnvironment } from "./shell-environment";

describe("filterAppServerEnvironment", () => {
  it("keeps talos variables and minimal process variables", () => {
    expect(
      filterAppServerEnvironment({
        TALOS_HOME: "/tmp/talos",
        TALOS_LLM_ANTHROPIC_API_KEY: "sk-test",
        HOME: "/Users/example",
        PATH: "/usr/bin",
        SHELL: "/bin/zsh",
        TMPDIR: "/tmp",
        USER: "example",
        ANTHROPIC_API_KEY: "ignored",
        APPLE_INTERNAL: "ignored",
      }),
    ).toEqual({
      TALOS_HOME: "/tmp/talos",
      TALOS_LLM_ANTHROPIC_API_KEY: "sk-test",
      HOME: "/Users/example",
      PATH: "/usr/bin",
      SHELL: "/bin/zsh",
      TMPDIR: "/tmp",
      USER: "example",
    });
  });
});

describe("resolveAppServerEnvironment", () => {
  it("does not read login shell or dotenv values", () => {
    const env = resolveAppServerEnvironment({
      baseEnv: {
        TALOS_HOME: "/tmp/talos",
        ANTHROPIC_API_KEY: "ignored",
      },
    });

    expect(env).toEqual({ TALOS_HOME: "/tmp/talos" });
  });

  it("uses packaged resources as the release config template source", () => {
    const env = resolveAppServerEnvironment({
      baseEnv: {
        HOME: "/Users/example",
      },
      packagedResourcesPath: "/Applications/Talos.app/Contents/Resources",
    });

    expect(env).toMatchObject({
      HOME: "/Users/example",
      TALOS_RESOURCE_DIR: "/Applications/Talos.app/Contents/Resources",
    });
  });

  it("keeps explicit packaged Talos config overrides", () => {
    const env = resolveAppServerEnvironment({
      baseEnv: {
        TALOS_CONFIG: "/tmp/custom.yaml",
        TALOS_RESOURCE_DIR: "/tmp/resources",
      },
      packagedResourcesPath: "/Applications/Talos.app/Contents/Resources",
    });

    expect(env).toMatchObject({
      TALOS_CONFIG: "/tmp/custom.yaml",
      TALOS_RESOURCE_DIR: "/tmp/resources",
    });
  });
});
