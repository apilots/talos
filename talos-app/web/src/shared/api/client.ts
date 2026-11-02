import { createBrowserTalosClient } from "@talos/sdk-browser";

import { connectionScope } from "@/shared/store/connection-scope";
import { bindComputerTarget } from "./computer-target";
import { createDesktopTalosClient, getDesktopBridge } from "./desktop-client";
import { resolveServerBaseUrl } from "./server-base-url";
import { useAppStore, getConnectionSettings } from "@/shared/store/app-store";

type BrowserTalosClient = ReturnType<typeof createBrowserTalosClient>;
type Methods<T, K extends keyof T> = Pick<T, K>;

/** Transport-neutral API surface consumed by the Web application. */
export type TalosWebClient = {
  trajectory: BrowserTalosClient["trajectory"];
  sync: Methods<BrowserTalosClient["sync"], "status" | "enable" | "disable" | "metrics">;
  computers: Methods<BrowserTalosClient["computers"], "list" | "read" | "rename" | "setStatus" | "delete">;
  auth: Methods<BrowserTalosClient["auth"], "logout">;
  user: Methods<BrowserTalosClient["user"], "current">;
  profile: Methods<BrowserTalosClient["profile"], "read">;
  llm: Methods<BrowserTalosClient["llm"], "get" | "update">;
  config: Methods<BrowserTalosClient["config"], "get" | "update">;
  models: Methods<BrowserTalosClient["models"], "list">;
  sessions: Methods<
    BrowserTalosClient["sessions"],
    | "list"
    | "get"
    | "patch"
    | "close"
    | "delete"
    | "cancel"
    | "compact"
    | "clear"
    | "activeTurn"
    | "history"
    | "recoverTimeline"
  >;
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

function createDefaultBrowserClient() {
  const desktopBridge = getDesktopBridge();
  if (desktopBridge) return createDesktopTalosClient(desktopBridge, getAuthTokenForRequests);

  return createBrowserTalosClient({
    baseUrl: resolveServerBaseUrl(),
    getToken: getAuthTokenForRequests,
  });
}

function getAuthTokenForRequests(): string | undefined {
  const connection = getConnectionSettings();
  if (connection.authStatus === "open") {
    return undefined;
  }
  return connection.authToken || undefined;
}

export function setTalosClientFactory(factory: TalosClientFactory) {
  clientFactory = factory;
}

export function resetTalosClientFactory() {
  clientFactory = undefined;
}

export function getTalosClient(nodeId?: string) {
  const state = useAppStore.getState();
  const target = nodeId ?? state.selectedComputers[connectionScope(state.connection)];
  const client = (clientFactory ?? createDefaultBrowserClient)();
  return target !== undefined ? bindComputerTarget(client, target) : client;
}
