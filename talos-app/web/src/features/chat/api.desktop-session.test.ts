// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { DESKTOP_CLOUD_ORIGIN } from "@talos/sdk-app-server/desktop";
import { connectionScope } from "@/shared/store/connection-scope";
import { useAppStore } from "@/shared/store/app-store";
import { productQueries, sessionDetailKey } from "@/shared/api/product-queries";
import { getSession, updateSessionPermissionMode } from "./api";

describe("Desktop session routing", () => {
  function desktopAccount() {
    const invoke = vi.fn<(method: string) => Promise<unknown>>();
    const http = vi.fn<typeof fetch>();
    Object.defineProperty(window, "talos", { configurable: true, value: { invoke } });
    vi.stubGlobal("fetch", http);
    useAppStore.getState().setConnection({
      userId: "alice", authRole: "user", accountType: "Free", authStatus: "authenticated", authToken: "alice-token",
    });
    const scope = connectionScope(useAppStore.getState().connection);
    useAppStore.getState().selectComputer(scope, "remote-computer");
    invoke.mockResolvedValue({ status: "success", data: { nodeId: "local-computer" } });
    return { invoke, http };
  }

  afterEach(() => {
    productQueries.clear();
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "talos");
    useAppStore.setState(useAppStore.getInitialState());
  });

  it("opens a standard Web chat and changes its permission on the Server when Desktop Sync is off", async () => {
    const { invoke, http } = desktopAccount();
    const session = { id: "web-chat", sessionKind: "standard", effectiveMode: "askForApproval" };

    http.mockImplementation((_url, request) => Promise.resolve(Response.json({ status: "success", data: { session: {
      ...session, effectiveMode: request?.method === "PATCH" ? "approveForMe" : session.effectiveMode,
    } } })));

    const scope = connectionScope(useAppStore.getState().connection);
    const detail = await productQueries.fetchQuery({
      queryKey: sessionDetailKey(scope, session.id), queryFn: () => getSession(session.id),
    });
    expect(detail.session).toEqual(session);
    await expect(updateSessionPermissionMode(session.id, "approveForMe")).resolves.toEqual({ ...session, effectiveMode: "approveForMe" });

    expect(invoke.mock.calls.map(([method]) => method)).toEqual(["desktop/workerIdentity", "desktop/workerIdentity"]);
    expect(http.mock.calls.map(([, request]) => request?.method)).toEqual(["GET", "PATCH"]);
    expect(http).toHaveBeenLastCalledWith(DESKTOP_CLOUD_ORIGIN + "/v1/sessions/web-chat", expect.objectContaining({ method: "PATCH" }));
  });

  it("returns the Server's permission denial for a downgraded user whose local session detail is restricted", async () => {
    const { invoke, http } = desktopAccount();

    const denied = { code: "permissionDenied", message: "Server read denied.", localization: null };
    http.mockResolvedValue(Response.json({ status: "error", error: denied }, { status: 403 }));

    await expect(updateSessionPermissionMode("former-talo", "approveForMe")).rejects.toMatchObject({ apiError: denied });

    expect(invoke.mock.calls.map(([method]) => method)).toEqual(["desktop/workerIdentity"]);
    expect(http).toHaveBeenCalledTimes(1);
    expect(http).toHaveBeenCalledWith(DESKTOP_CLOUD_ORIGIN + "/v1/sessions/former-talo", expect.objectContaining({
      method: "PATCH", headers: expect.objectContaining({ Authorization: "Bearer alice-token" }) as unknown,
    }));
  });
});
