// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { DESKTOP_CLOUD_ORIGIN } from "@talos/sdk-app-server/desktop";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { getTalosClient, type TalosWebClient } from "./client";
import { MANAGEMENT_LOOKUP_TIMEOUT_MS } from "./desktop-product-owner";
import { productQueries, managedWorkerKey } from "./product-queries";

afterEach(() => {
  productQueries.clear();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(window, "talos");
  useAppStore.setState(useAppStore.getInitialState());
});

function desktopAccount() {
  const invoke = vi.fn().mockResolvedValue({ status: "success", data: {} });
  const prepareCloud = vi.fn().mockResolvedValue({ previewId: "server-preview", entryPath: "report.pdf" });
  Object.defineProperty(window, "talos", { configurable: true, value: { invoke, preview: { prepareCloud } } });
  const connection = {
    ...useAppStore.getInitialState().connection, userId: "alice", accountType: "Max" as const,
    authStatus: "authenticated" as const, authToken: "alice-token",
  };
  const scope = connectionScope(connection);
  useAppStore.setState({ connection, selectedComputers: { [scope]: "embedded" } });
  const http = vi.fn().mockImplementation(async () => Response.json({ status: "success", data: { ticket: "one-use" } }));
  vi.stubGlobal("fetch", http);
  return { http, invoke, prepareCloud, scope };
}

it.each([
  ["default", () => getTalosClient()],
  ["selected Worker", () => getTalosClient("embedded")],
  ["resolved Worker", () => getTalosClient({ nodeId: "embedded" })],
  ["explicit Server", () => getTalosClient({ service: "server" })],
] satisfies [string, () => TalosWebClient | Promise<TalosWebClient>][])(
  "uses the account Server directory for %s client reads and mutations",
  async (_name, clientForTarget) => {
    const { http, invoke } = desktopAccount();
    const directory = { data: [{ nodeId: "cloud-worker", displayName: "Server name" }], nextCursor: null, total: 1 };
    http.mockResolvedValue(Response.json({ status: "success", data: directory }));
    const client = await clientForTarget();

    expect(await client.computers.list({ query: "Server name", limit: 10 })).toEqual(directory);
    http.mockImplementation(async () => Response.json({ status: "success", data: directory.data[0] }));
    expect(await client.computers.read({ nodeId: "cloud-worker" })).toEqual(directory.data[0]);
    await client.computers.rename({ nodeId: "cloud-worker", displayName: "Renamed" });
    await client.computers.setStatus({ nodeId: "cloud-worker", status: "disabled" });
    await client.computers.delete({ nodeId: "cloud-worker" });

    expect(http.mock.calls.map(([url, init]) => [url, init?.method ?? "GET"])).toEqual([
      [DESKTOP_CLOUD_ORIGIN + "/v1/computers?query=Server+name&limit=10", "GET"],
      [DESKTOP_CLOUD_ORIGIN + "/v1/computers/cloud-worker", "GET"],
      [DESKTOP_CLOUD_ORIGIN + "/v1/computers/cloud-worker/name", "PATCH"],
      [DESKTOP_CLOUD_ORIGIN + "/v1/computers/cloud-worker/status", "PUT"],
      [DESKTOP_CLOUD_ORIGIN + "/v1/computers/cloud-worker", "DELETE"],
    ]);
    for (const [, init] of http.mock.calls) {
      expect(init.headers).toMatchObject({ Authorization: "Bearer alice-token" });
    }
    expect(invoke).not.toHaveBeenCalled();
  },
);

it.each(["list", "read", "readManagedWorker"] as const)(
  "bounds Desktop directory %s calls without falling back to the local database",
  async method => {
    vi.useFakeTimers();
    const { http, invoke } = desktopAccount();
    http.mockImplementation(() => new Promise(() => {}));
    const { computers } = getTalosClient();
    const pending = method === "list" ? computers.list()
      : method === "read" ? computers.read({ nodeId: "cloud-worker" })
        : computers.readManagedWorker();
    const rejected = expect(pending).rejects.toMatchObject({ code: AppProtocol.ErrorCode.Timeout });
    await vi.advanceTimersByTimeAsync(MANAGEMENT_LOOKUP_TIMEOUT_MS);
    await rejected;
    expect(http.mock.calls[0][1].signal.aborted).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  },
);

it.each(["list", "read"] as const)("preserves caller cancellation for directory %s", async method => {
  const { http } = desktopAccount();
  const controller = new AbortController();
  const { computers } = getTalosClient();
  if (method === "list") await computers.list({}, controller.signal);
  else await computers.read({ nodeId: "cloud-worker" }, controller.signal);
  expect(http).toHaveBeenLastCalledWith(
    DESKTOP_CLOUD_ORIGIN + (method === "list" ? "/v1/computers" : "/v1/computers/cloud-worker"),
    expect.objectContaining({ signal: controller.signal }),
  );
});

it("propagates authoritative Server directory errors without a local fallback", async () => {
  const { http, invoke } = desktopAccount();
  http.mockRejectedValue(new TalosApiError({
    code: AppProtocol.ErrorCode.ResourceNotFound, message: "Computer not found.", localization: null,
  }));
  await expect(getTalosClient().computers.read({ nodeId: "removed-worker" })).rejects.toMatchObject({
    code: AppProtocol.ErrorCode.ResourceNotFound,
  });
  expect(invoke).not.toHaveBeenCalled();
});

it("opens the managed desktop through the authenticated Server SDK and keeps the selected local Worker on stdio", async () => {
  const { http, invoke, scope } = desktopAccount();
  productQueries.setQueryData(managedWorkerKey(scope), { enabled: true, instance: { nodeId: "talo", conversationSessionId: "managed-chat" } });
  const grant = await getTalosClient("talo").desktop.open({
    nodeId: "talo", targetId: "managed", viewerId: "viewer", generation: "1", inputMode: "readOnly", controllerId: null, inputEpoch: null,
  });
  expect(grant.ticket).toBe("server:one-use");
  expect(http).toHaveBeenCalledWith(DESKTOP_CLOUD_ORIGIN + "/v1/desktop/open", expect.objectContaining({
    headers: expect.objectContaining({ Authorization: "Bearer alice-token" }),
  }));
  await getTalosClient("embedded").models.list();
  expect(invoke).toHaveBeenCalledWith("model/list", { nodeId: "embedded" }, expect.anything());
});

it("prepares an attachment on its Server and uses the issued native grant for presentation and release", async () => {
  const { invoke, prepareCloud } = desktopAccount();
  invoke.mockResolvedValueOnce({ status: "success", data: { session: { id: "managed-chat", sessionKind: "talo" } } });
  const { files } = await getTalosClient({ sessionId: "managed-chat", readOnly: true });
  const source = { kind: "file", fileId: "cloud-file" } satisfies AppProtocol.FilePreviewSource;
  const descriptor = await files.preview.prepare({ source });
  expect(prepareCloud).toHaveBeenCalledWith({ source }, { credential: { type: "existing_session_token", token: "alice-token" } });
  expect(files.preview.resourceUrl(descriptor)).toBe("talos-preview://server-preview/report.pdf");
  await files.preview.close({ previewId: descriptor.previewId });
  expect(invoke).toHaveBeenCalledWith("file/preview/close", { previewId: "server-preview" }, expect.anything());
});
