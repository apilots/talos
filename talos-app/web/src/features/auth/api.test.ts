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

  it("unwraps successful HTTP authentication responses", async () => {
    const user = {
      user_id: "usr_1",
      username: "cloud-user",
      email: "cloud-user@example.com",
      role: "user" as const,
      type: "free" as const,
      disabled: false,
      created_at: 1,
      updated_at: 1,
    };
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(new Response(JSON.stringify({ status: "success", data: { user } })))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        status: "success",
        data: {
          login_id: "login-1",
          login_url: "https://cloud.talos.love/device",
          user_code: "ABCD-EFGH",
          expires_at_ms: 2,
          interval_ms: 1,
        },
      })))
      .mockResolvedValueOnce(new Response(JSON.stringify({
        status: "success",
        data: { status: "authenticated", user, access_token: "token", token_type: "Bearer" },
      })));

    await expect(getCurrentUserWithToken("token")).resolves.toEqual(user);
    await expect(startCloudLogin()).resolves.toMatchObject({ login_id: "login-1" });
    await expect(getCloudLoginStatus("login-1")).resolves.toMatchObject({
      status: "authenticated",
      access_token: "token",
    });
  });
});
