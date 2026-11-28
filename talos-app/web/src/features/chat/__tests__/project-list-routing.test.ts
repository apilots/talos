// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { DESKTOP_CLOUD_ORIGIN } from "@talos/sdk-app-server/desktop";
import { AppProtocol } from "@talos/sdk-browser";
import { getTalosClient } from "@/shared/api/client";
import { managedWorkerKey, productQueries } from "@/shared/api/product-queries";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { listProjects } from "../api";

afterEach(() => {
  productQueries.clear();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(window, "talos");
  useAppStore.setState(useAppStore.getInitialState());
});

function desktopAccount() {
  const invoke = vi.fn<(method:string,...args:unknown[])=>Promise<unknown>>().mockResolvedValue({ status: "success", data: { nodeId: "local-computer" } });
  Object.defineProperty(window, "talos", { configurable: true, value: { invoke } });
  const connection = {
    ...useAppStore.getInitialState().connection, userId: "alice", accountType: "Max" as const,
    authStatus: "authenticated" as const, authToken: "alice-token",
  };
  const scope = connectionScope(connection);
  useAppStore.setState({ connection });
  const http = vi.fn();
  vi.stubGlobal("fetch", http);
  return { http, invoke, scope };
}

it.each(["offline", "removed"])("keeps remote project failure authoritative when its Server is %s", async state => {
  const { http, invoke } = desktopAccount();
  if (state === "offline") http.mockRejectedValue(new TypeError("Server unreachable"));
  else http.mockResolvedValue(Response.json({ status: "error", error: {
    code: AppProtocol.ErrorCode.ResourceNotFound, message: "Computer not found.", localization: null,
  } }, { status: 404 }));
  await expect(listProjects("retained-node")).rejects.toBeDefined();
  expect(invoke.mock.calls.map(([method]) => method)).toEqual(["desktop/workerIdentity"]);
});

it("keeps a directory not-found authoritative for Computer details even when Desktop can answer", async () => {
  const { http, invoke } = desktopAccount();
  http.mockResolvedValueOnce(Response.json({ status: "error", error: {
    code: AppProtocol.ErrorCode.ResourceNotFound, message: "Computer not found.", localization: null,
  } }, { status: 404 }));


  const readComputer = async () => {
    const client = await getTalosClient({ nodeId: "removed-node", readOnly: true });
    return client.computers.read({ nodeId: "removed-node" });
  };
  await expect(readComputer()).rejects.toMatchObject({ code: AppProtocol.ErrorCode.ResourceNotFound });
  expect(invoke.mock.calls.map(([method]) => method)).toEqual(["desktop/workerIdentity"]);
});

it("does not fall back to local project metadata after a directory permission failure", async () => {
  const { http, invoke } = desktopAccount();
  http.mockResolvedValueOnce(Response.json({ status: "error", error: {
    code: AppProtocol.ErrorCode.PermissionDenied, message: "Access denied.", localization: null,
  } }, { status: 403 }));

  await expect(listProjects("restricted-node")).rejects.toMatchObject({ code: AppProtocol.ErrorCode.PermissionDenied });
  expect(invoke.mock.calls.map(([method]) => method)).toEqual(["desktop/workerIdentity"]);
});

it("keeps managed project metadata on its Server and sends the requested Computer ID", async () => {
  const { http, invoke, scope } = desktopAccount();
  productQueries.setQueryData(managedWorkerKey(scope), { instance: { nodeId: "managed-node" } });
  http.mockResolvedValueOnce(Response.json({ status: "success", data: { projects: [] } }));

  await expect(listProjects("managed-node")).resolves.toEqual([]);
  expect(http).toHaveBeenCalledExactlyOnceWith(DESKTOP_CLOUD_ORIGIN + "/v1/projects?nodeId=managed-node", expect.anything());
  expect(invoke).not.toHaveBeenCalled();
});
