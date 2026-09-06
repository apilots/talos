// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { getCloudLoginStatus, getCurrentUserWithToken, getGatewayHealth, logoutUser, startCloudLogin } from "./api";
import {
  resetTalosClientFactory,
  setTalosClientFactory,
  type TalosWebClient,
} from "@/shared/api/client";

describe("auth api", () => {
  beforeEach(() => {
    window.sessionStorage.clear();
  });

  afterEach(() => {
    resetTalosClientFactory();
    vi.restoreAllMocks();
  });

  it("clears the browser token after logout", async () => {
    window.sessionStorage.setItem("talos-web-auth-token", "web-token");
    const logout = vi.fn().mockResolvedValue(undefined);
    setTalosClientFactory(() => ({ auth: { logout } } as unknown as TalosWebClient));

    await logoutUser();

    expect(logout).toHaveBeenCalledTimes(1);
    expect(window.sessionStorage.getItem("talos-web-auth-token")).toBeNull();
  });

  it.each([
    ["health", () => getGatewayHealth()],
    ["current user", () => getCurrentUserWithToken("token")],
    ["login start", () => startCloudLogin()],
    ["login status", () => getCloudLoginStatus("login-1")],
  ] as const)("isolates malformed %s response diagnostics", async (_name, request) => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("SECRET_AUTH_BODY"));
    await expect(request()).rejects.toMatchObject({
      code: "internalError",
      message: "The backend returned invalid JSON.",
      cause: expect.any(SyntaxError),
    });
  });
});
