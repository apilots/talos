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
  const invoke = vi.fn();
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

it.each(["offline", "removed"])("reads retained projects over IPC when the Computer directory is %s", async (state) => {
  const { http, invoke } = desktopAccount();
  if (state === "offline") http.mockRejectedValueOnce(new TypeError("Server unreachable"));
  else http.mockResolvedValueOnce(Response.json({ status: "error", error: {
    code: AppProtocol.ErrorCode.ResourceNotFound, message: "Computer not found.", localization: null,
  } }, { status: 404 }));
  invoke.mockResolvedValueOnce({ status: "success", data: {
    projects: [{ project_id: "retained-project", nodeId: "retained-node", directory: "/workspace/project" }],
  } });

  await expect(listProjects("retained-node")).resolves.toMatchObject([{ id: "retained-project", nodeId: "retained-node" }]);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("project/list", { nodeId: "retained-node" }, expect.anything());
});

it("keeps a directory not-found authoritative for Computer details even when Desktop can answer", async () => {
  const { http, invoke } = desktopAccount();
  http.mockResolvedValueOnce(Response.json({ status: "error", error: {
    code: AppProtocol.ErrorCode.ResourceNotFound, message: "Computer not found.", localization: null,
  } }, { status: 404 }));
  invoke.mockResolvedValue({ status: "success", data: { nodeId: "removed-node", displayName: "Cached Computer" } });

  const readComputer = async () => {
    const client = await getTalosClient({ nodeId: "removed-node", readOnly: true });
    return client.computers.read({ nodeId: "removed-node" });
  };
  await expect(readComputer()).rejects.toMatchObject({ code: AppProtocol.ErrorCode.ResourceNotFound });
  expect(invoke).not.toHaveBeenCalled();
});

it("does not fall back to local project metadata after a directory permission failure", async () => {
  const { http, invoke } = desktopAccount();
  http.mockResolvedValueOnce(Response.json({ status: "error", error: {
    code: AppProtocol.ErrorCode.PermissionDenied, message: "Access denied.", localization: null,
  } }, { status: 403 }));

  await expect(listProjects("restricted-node")).rejects.toMatchObject({ code: AppProtocol.ErrorCode.PermissionDenied });
  expect(invoke).not.toHaveBeenCalled();
});

it("keeps managed project metadata on its Server and sends the requested Computer ID", async () => {
  const { http, invoke, scope } = desktopAccount();
  productQueries.setQueryData(managedWorkerKey(scope), { instance: { nodeId: "managed-node" } });
  http.mockResolvedValueOnce(Response.json({ status: "success", data: { projects: [] } }));

  await expect(listProjects("managed-node")).resolves.toEqual([]);
  expect(http).toHaveBeenCalledExactlyOnceWith(DESKTOP_CLOUD_ORIGIN + "/v1/projects?nodeId=managed-node", expect.anything());
  expect(invoke).not.toHaveBeenCalled();
});
