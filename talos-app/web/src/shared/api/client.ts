import { createBrowserTalosClient, TalosApiError } from "@talos/sdk-browser";

import { DESKTOP_CLOUD_ORIGIN } from "@talos/sdk-app-server/desktop";
import { connectionScope } from "@/shared/store/connection-scope";
import { bindComputerTarget } from "./computer-target";
import { actorCredentialsFromToken, createDesktopTalosClient, getDesktopBridge } from "./desktop-client";
import { canUseTalo } from "@/shared/types/account";
import { desktopProductOwner, withManagementTimeout, type ProductTarget } from "./desktop-product-owner";
import { productQueries, type SessionDetail } from "./product-queries";
import { resolveServerBaseUrl } from "./server-base-url";
import { useAppStore, getConnectionSettings } from "@/shared/store/app-store";

type BrowserTalosClient = ReturnType<typeof createBrowserTalosClient>;
type Methods<T, K extends keyof T> = Pick<T, K>;

/** Transport-neutral API surface consumed by the Web application. */
export type TalosWebClient = {
  trajectory: BrowserTalosClient["trajectory"];
  automation: BrowserTalosClient["automation"];
  builtinJobs: BrowserTalosClient["builtinJobs"];
  sync: Methods<BrowserTalosClient["sync"], "status" | "enable" | "disable" | "metrics">;
  computers: Methods<
    BrowserTalosClient["computers"],
    "list" | "read" | "rename" | "setStatus" | "delete" |
    "readManagedWorker" | "createManagedWorker" | "startManagedWorker" | "stopManagedWorker" | "resetManagedWorker" | "deleteManagedWorker"
  >;
  auth: Methods<BrowserTalosClient["auth"], "logout">;
  user: Methods<BrowserTalosClient["user"], "current">;
  profile: Methods<BrowserTalosClient["profile"], "read">;
  llm: Methods<BrowserTalosClient["llm"], "get" | "update">;
  config: Methods<BrowserTalosClient["config"], "get" | "update">;
  models: Methods<BrowserTalosClient["models"], "list">;
  sessions: Methods<
    BrowserTalosClient["sessions"],
    | "list"
    | "patch"
    | "close"
    | "delete"
    | "cancel"
    | "compact"
    | "clear"
    | "activeTurn"
    | "history"
    | "recoverTimeline"
  > & { get: (request: { sessionId: string }) => Promise<SessionDetail> };
  messageQueue: Methods<
    BrowserTalosClient["messageQueue"],
    "list" | "add" | "update" | "delete" | "reorder" | "start"
  >;
  projects: Methods<BrowserTalosClient["projects"], "list" | "create" | "patch" | "delete">;
  files: Methods<BrowserTalosClient["files"], "upload" | "list" | "read" | "content" | "update" | "delete" | "preview">;
  fs: {
    directory: Methods<BrowserTalosClient["fs"]["directory"], "read" | "create">;
  };
  memory: Methods<BrowserTalosClient["memory"], "list" | "get" | "put" | "search" | "getEntry">;
  workspaceDocs: Methods<BrowserTalosClient["workspaceDocs"], "info" | "get" | "put">;
  skills: Methods<BrowserTalosClient["skills"], "list" | "suggest" | "read">;
  desktop: Methods<BrowserTalosClient["desktop"], "open" | "connect">;
  browser: Methods<BrowserTalosClient["browser"], "operate">;
  mcp: Methods<BrowserTalosClient["mcp"], "list" | "import" | "startAuthentication" | "readAuthentication" | "updateCredentials">;
  plugin: Methods<
    BrowserTalosClient["plugin"],
    "search" | "list" | "read" | "install" | "update" | "uninstall" | "enable" | "disable"
  > & {
    sources: Methods<BrowserTalosClient["plugin"]["sources"], "add">;
  };
  turns: Methods<BrowserTalosClient["turns"], "start" | "retry" | "input" | "events">;
};
export type TalosClientFactory = () => TalosWebClient;

let clientFactory: TalosClientFactory | undefined;

/** Use the authenticated management Server in both shells. Desktop owns its fixed HTTP relay. */
export function getServerTalosClient() {
  const scope = connectionScope(getConnectionSettings());
  const desktop = getDesktopBridge();
  const client = createBrowserTalosClient({
    baseUrl: desktop ? DESKTOP_CLOUD_ORIGIN : resolveServerBaseUrl(),
    getToken: () => getAuthTokenForRequests(scope),
    getUserId: () => {
      const connection = getConnectionSettings();
      return connectionScope(connection) === scope && connection.authStatus === "authenticated" ? connection.userId : undefined;
    },
  });
  if (desktop) {
    const directory = client.computers;
    client.computers = {
      ...directory,
      list: (request, signal) => signal
        ? directory.list(request, signal)
        : withManagementTimeout(timeout => directory.list(request, timeout)),
      read: (request, signal) => signal
        ? directory.read(request, signal)
        : withManagementTimeout(timeout => directory.read(request, timeout)),
      readManagedWorker: signal => signal
        ? directory.readManagedWorker(signal)
        : withManagementTimeout(timeout => directory.readManagedWorker(timeout)),
    };
  }
  return client;
}

function getAuthTokenForRequests(scope: string): string | undefined {
  const connection = getConnectionSettings();
  if (connectionScope(connection) !== scope) throw new DOMException("The account changed.", "AbortError");
  return connection.authStatus === "open" ? undefined : connection.authToken || undefined;
}

export function setTalosClientFactory(factory: TalosClientFactory) {
  clientFactory = factory;
}

export function resetTalosClientFactory() {
  clientFactory = undefined;
}

type ClientTarget = ProductTarget | { service: "server" };

export function getTalosClient(nodeId?: string): TalosWebClient;
export function getTalosClient(target: ClientTarget): Promise<TalosWebClient>;
export function getTalosClient(target?: string | ClientTarget): TalosWebClient | Promise<TalosWebClient> {
  const state = useAppStore.getState();
  const scope = connectionScope(state.connection);
  const selected = state.selectedComputers[scope];
  const request = typeof target === "object" ? target : undefined;
  const nodeId = typeof target === "object"
    ? ("nodeId" in target ? target.nodeId : undefined)
    : target ?? selected;
  const bind = (client: TalosWebClient) => nodeId ? bindComputerTarget(client, nodeId) : client;
  const bridge = getDesktopBridge();
  if (clientFactory || !bridge) {
    const client = bind(clientFactory ? clientFactory() : getServerTalosClient());
    return request ? Promise.resolve(client) : client;
  }

  const token = () => getAuthTokenForRequests(scope);
  const sdk = getServerTalosClient();
  // Account directory ownership is independent of the selected execution transport.
  const local = { ...createDesktopTalosClient(bridge, token), computers: sdk.computers };
  const cloud: TalosWebClient = {
    ...sdk,
    desktop: {
      open: async params => {
        const grant = await sdk.desktop.open(params);
        return { ...grant, ticket: `server:${grant.ticket}` };
      },
      connect: local.desktop.connect,
    },
    files: { ...sdk.files, preview: {
      ...local.files.preview,
      prepare: async params => {
        const actor = actorCredentialsFromToken(token());
        if (!actor || !bridge.preview) throw new TalosApiError({
          code: "serviceUnavailable", message: "The authenticated preview service is unavailable.", localization: null,
        });
        return bridge.preview.prepareCloud(params, actor);
      },
    } },
  };
  if (request?.service === "server") return Promise.resolve(cloud);
  const routing = desktopProductOwner(local, cloud, productQueries, scope, canUseTalo(state.connection),
    state.connection.authStatus === "authenticated", state.connection.controlPlaneEndpoint);
  if (request) return routing.resolve(request).then(bind);
  const host = { ...local, sessions: { ...local.sessions, get: ({ sessionId }: { sessionId: string }) => routing.readSession(sessionId) } };
  return nodeId ? bindComputerTarget(host, nodeId, id => routing.resolve({ nodeId: id })) : host;
}
