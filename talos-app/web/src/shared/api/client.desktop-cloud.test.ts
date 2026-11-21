// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { DESKTOP_CLOUD_ORIGIN } from "@talos/sdk-app-server/desktop";
import { type AppProtocol } from "@talos/sdk-browser";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";
import { getTalosClient } from "./client";
import { productQueries, managedWorkerKey } from "./product-queries";

afterEach(() => {
  productQueries.clear();
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
  productQueries.setQueryData(managedWorkerKey(scope), { enabled: true, instance: { nodeId: "talo", conversationSessionId: "managed-chat" } });
  const http = vi.fn().mockImplementation(async () => Response.json({ status: "success", data: { ticket: "one-use" } }));
  vi.stubGlobal("fetch", http);
  return { http, invoke, prepareCloud };
}

it("opens the managed desktop through the authenticated Server SDK and keeps the selected local Worker on stdio", async () => {
  const { http, invoke } = desktopAccount();
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
  const { files } = await getTalosClient({ sessionId: "managed-chat", readOnly: true });
  const source = { kind: "file", fileId: "cloud-file" } satisfies AppProtocol.FilePreviewSource;
  const descriptor = await files.preview.prepare({ source });
  expect(prepareCloud).toHaveBeenCalledWith({ source }, { credential: { type: "existing_session_token", token: "alice-token" } });
  expect(files.preview.resourceUrl(descriptor)).toBe("talos-preview://server-preview/report.pdf");
  await files.preview.close({ previewId: descriptor.previewId });
  expect(invoke).toHaveBeenCalledWith("file/preview/close", { previewId: "server-preview" }, expect.anything());
});
