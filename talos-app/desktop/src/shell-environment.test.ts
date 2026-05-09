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
});
