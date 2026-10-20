import type { AppProtocol } from "@talos/sdk-core";

type ParamGuard = (params: unknown) => boolean;
// Enrollment and credential delivery belong to the trusted host, never the renderer bridge.
export type DesktopRendererMethod = Exclude<
  AppProtocol.ClientMethod, "initialize" | "computer/create" | "computer/rotateCredential"
>;

const mcpDefinitionRef = (value: unknown) => isRecord(value) && ((value.type === "plugin" && typeof value.installId === "string") || (value.type === "config" && typeof value.name === "string"));
const mcpCredentials = (value: unknown) => isRecord(value) && isRecord(value.authentication) && (value.authentication.type === "none" || value.authentication.type === "bearer" || (value.authentication.type === "header" && typeof value.authentication.name === "string")) && (value.token === undefined || value.token === null || typeof value.token === "string") && [value.env, value.headers].every((map) => map === undefined || map === null || (isRecord(map) && Object.values(map).every((entry) => typeof entry === "string")));
const browserParams = (params:unknown) => isRecord(params) && typeof params.nodeId === "string" && isRecord(params.operation) && ["read","selectTarget","connect","disconnect","pause","resume","openLocal","takeControl","releaseControl"].includes(String(params.operation.kind));
const desktopOpenParams = (params:unknown) => isRecord(params) && ["nodeId","targetId","viewerId","generation"].every(key=>typeof params[key] === "string") && (
  params.inputMode==="readOnly" ? params.controllerId===null && params.inputEpoch===null :
  params.inputMode==="control" && typeof params.controllerId==="string" && Number.isSafeInteger(params.inputEpoch) && Number(params.inputEpoch)>=0
);
const emptyParams = (params: unknown) => isRecord(params);
const sessionParams = (params: unknown) => hasString(params, "sessionId");
const projectParams = (params: unknown) => hasString(params, "project_id");
const fsDirectoryReadParams = (params: unknown) =>
  isRecord(params) && (params.path === undefined || params.path === null || typeof params.path === "string");
const fsDirectoryCreateParams = (params: unknown) =>
  hasString(params, "parent") && hasString(params, "name");
const fileUploadParams = (params: unknown) =>
  isRecord(params) && isRecord(params.request) && typeof params.dataBase64 === "string";
const fileIdParams = (params: unknown) => hasString(params, "fileId");
const configGetParams = (params: unknown) =>
  isRecord(params) && (params.path === "llm" || params.path === "webSearch");
const configWriteParams = (params: unknown) =>
  isRecord(params) &&
  isRecord(params.value) &&
  (params.value.type === "llm" || params.value.type === "webSearch") &&
  isRecord(params.value.value);
const authLoginStartParams = (params: unknown) =>
  isRecord(params) &&
  hasString(params, "client_name") &&
  typeof params.client_kind === "string" &&
  ["web", "desktop", "tui", "cli"].includes(params.client_kind);
const authLoginSessionParams = (params: unknown) => isRecord(params) && hasString(params, "login_id");
const skillSelector = (selector: unknown) =>
  isRecord(selector) &&
  (selector.name === undefined || typeof selector.name === "string") &&
  (selector.path === undefined || typeof selector.path === "string") &&
  (typeof selector.name === "string" || typeof selector.path === "string");
const skillSelectorParams = (params: unknown) => isRecord(params) && skillSelector(params.selector);
const stringArray = (value: unknown) =>
  Array.isArray(value) && value.every((item) => typeof item === "string");
const marketplaceSourceInput = (source: unknown) =>
  isRecord(source) &&
  ((source.kind === "git" &&
    typeof source.url === "string" &&
    typeof source.reference === "string" &&
    stringArray(source.sparse_paths)) ||
    (source.kind === "local_folder" &&
      typeof source.path === "string" &&
      stringArray(source.sparse_paths)));
const pluginSourceAddParams = (params: unknown) =>
  isRecord(params) &&
  marketplaceSourceInput(params.source) &&
  (params.refresh === undefined || typeof params.refresh === "boolean");
const pluginSourceIdParams = (params: unknown) => isRecord(params) && hasString(params, "source_id");
const pluginReadParams = (params: unknown) => isRecord(params) && hasString(params, "plugin_id");
const pluginSearchParams = (params: unknown) =>
  isRecord(params) &&
  (params.query === null || typeof params.query === "string") &&
  (params.source_id === null || typeof params.source_id === "string") &&
  (params.kind === null || ["skill", "mcp_server", "package", "feature"].includes(String(params.kind))) &&
  (params.status === null ||
    ["any", "not_installed", "installed", "update_available", "conflict"].includes(String(params.status))) &&
  ["cache_only", "refresh_stale", "force_refresh"].includes(String(params.refresh)) &&
  (params.limit === null || typeof params.limit === "number");
const pluginKind = (value: unknown) =>
  value === null || value === undefined || ["skill", "mcp_server", "package", "feature"].includes(String(value));
const pluginListParams = (params: unknown) =>
  isRecord(params) &&
  pluginKind(params.kind) &&
  (params.status === null ||
    params.status === undefined ||
    ["installed", "disabled", "update_available", "source_missing", "conflict"].includes(String(params.status))) &&
  (params.source_id === null || params.source_id === undefined || typeof params.source_id === "string") &&
  typeof params.include_disabled === "boolean";
const pluginInstallParams = (params: unknown) => {
  if (!isRecord(params) || !isRecord(params.target) ||
      !(params.enable === null || params.enable === undefined || typeof params.enable === "boolean") ||
      !["reject", "replace"].includes(String(params.conflict_policy))) return false;
  const target = params.target;
  if (target.kind === "plugin") return typeof target.plugin_id === "string" &&
    (target.expected_source_commit === null || target.expected_source_commit === undefined || typeof target.expected_source_commit === "string");
  if (target.kind !== "manual_mcp_server" || !isRecord(target.server)) return false;
  const server = target.server;
  return typeof server.name === "string" && isRecord(server.transport) &&
    ((server.transport.type === "stdio" && typeof server.transport.command === "string") ||
     (server.transport.type === "streamable_http" && typeof server.transport.url === "string"));
};
const pluginInstallIdParams = (params: unknown) => isRecord(params) && hasString(params, "install_id");
const pluginUpdateParams = (params: unknown) =>
  isRecord(params) &&
  hasString(params, "install_id") &&
  (params.expected_source_commit === null ||
    params.expected_source_commit === undefined ||
    typeof params.expected_source_commit === "string") &&
  ["reject_local_changes", "replace_local_changes"].includes(String(params.conflict_policy));
const turnIdentity = (value: unknown) =>
  isRecord(value) && hasString(value, "sessionId") && hasString(value, "turnId");
const agentUserInput = (value: unknown) => isRecord(value) && Array.isArray(value.content);
const turnInput = (value: unknown) => {
  if (!isRecord(value)) {
    return false;
  }
  if (value.type === "message") {
    return agentUserInput(value.input);
  }
  if (value.type === "queuedMessage") {
    return typeof value.messageId === "string";
  }
  return value.type === "userActionResponse" &&
    typeof value.actionId === "string" &&
    isRecord(value.response) &&
    ["question", "sandboxApproval", "dynamicToolResult"].includes(String(value.response.type));
};
const turnInputParams = (params: unknown) =>
  isRecord(params) && turnIdentity(params.identity) && turnInput(params.input);
const messageQueueSessionParams = (params: unknown) => hasString(params, "sessionId");
const messageQueueMessageParams = (params: unknown) =>
  messageQueueSessionParams(params) && hasString(params, "messageId");

const DESKTOP_RENDERER_METHOD_GUARDS = {
  "sync/metrics": (params) => params == null,
  "sync/status": (params) => params == null,
  "sync/disable": (params) => params == null,
  "sync/enable": (params) => isRecord(params) && (params.claimLocalHistory === undefined || typeof params.claimLocalHistory === "boolean"),
  "desktop/workerIdentity": (params) => params === null || params === undefined,
  "desktop/workerEnroll": (params) => isRecord(params) && (params.nodeId === undefined || params.nodeId === null || typeof params.nodeId === "string"),
  "computer/list": (params) => isRecord(params) &&
    [params.query, params.cursor].every((value) => value == null || typeof value === "string") &&
    (params.limit == null || (typeof params.limit === "number" && Number.isInteger(params.limit) && params.limit >= 0)),
  "computer/delete": (params) => hasString(params, "nodeId"),
  "computer/read": (params) => hasString(params, "nodeId"),
  "computer/rename": (params) => hasString(params, "nodeId") && hasString(params, "displayName"),
  "computer/setStatus": (params) => hasString(params, "nodeId") && isRecord(params) &&
    (params.status === "enabled" || params.status === "disabled"),
  "auth/login/cancel": authLoginSessionParams,
  "auth/login/start": authLoginStartParams,
  "auth/login/status": authLoginSessionParams,
  "auth/logout": emptyParams,
  "auth/session/clear": emptyParams,
  "auth/session/read": emptyParams,
  "config/get": configGetParams,
  "config/update": configWriteParams,
  "config/delete": (params) => hasString(params, "path"),
  "config/reload": emptyParams,
  "event/subscribe": sessionParams,
  "event/unsubscribe": (params) => hasString(params, "subscriptionId"),
  "file/upload": fileUploadParams,
  "file/list": emptyParams,
  "file/read": fileIdParams,
  "file/content": fileIdParams,
  "file/update": fileIdParams,
  "file/delete": fileIdParams,
  "fs/directory/create": fsDirectoryCreateParams,
  "fs/directory/read": fsDirectoryReadParams,
  "memory/file/list": emptyParams,
  "memory/file/read": (params) => hasString(params, "date"),
  "memory/file/write": (params) => hasString(params, "date") && hasString(params, "content"),
  "memory/get": (params) => hasString(params, "memoryId"),
  "memory/search": (params) => hasString(params, "query"),
  "messageQueue/add": (params) =>
    messageQueueSessionParams(params) && isRecord(params) && agentUserInput(params.input),
  "messageQueue/list": messageQueueSessionParams,
  "messageQueue/update": (params) =>
    messageQueueMessageParams(params) && isRecord(params) && agentUserInput(params.input),
  "messageQueue/delete": messageQueueMessageParams,
  "messageQueue/reorder": (params) => messageQueueSessionParams(params) && hasArray(params, "messageIds"),
  "messageQueue/start": (params) =>
    messageQueueSessionParams(params) &&
    isRecord(params) &&
    (params.messageId === undefined || params.messageId === null || typeof params.messageId === "string"),
  "model/list": emptyParams,
  "project/create": (params) => hasString(params, "name") && hasString(params, "directory"),
  "project/delete": projectParams,
  "project/list": emptyParams,
  "project/patch": projectParams,
  "project/read": projectParams,
  "session/active_turn": sessionParams,
  "session/clear": sessionParams,
  "session/close": sessionParams,
  "session/compact": sessionParams,
  "session/delete": sessionParams,
  "session/history": sessionParams,
  "session/list": emptyParams,
  "session/patch": sessionParams,
  "session/get": sessionParams,
  "timeline/recover": sessionParams,
  "plugin/source/list": emptyParams,
  "plugin/source/add": pluginSourceAddParams,
  "plugin/source/refresh": pluginSourceIdParams,
  "plugin/source/remove": pluginSourceIdParams,
  "plugin/search": pluginSearchParams,
  "mcp/server/import": (params) => isRecord(params) && typeof params.json === "string" && params.json.length <= 256 * 1024 && (params.conflictPolicy === "reject" || params.conflictPolicy === "replace"),
  "mcp/server/auth/start": (params) => isRecord(params) && mcpDefinitionRef(params.server) && isRecord(params.config) && (params.clientSecret === undefined || typeof params.clientSecret === "string"),
  "mcp/server/auth/read": (params) => isRecord(params) && mcpDefinitionRef(params.server),
  "mcp/server/credentials/update": (params) => isRecord(params) && mcpDefinitionRef(params.server) && isRecord(params.change) && (params.change.type === "clear" || (params.change.type === "replace" && mcpCredentials(params.change.credentials))),
  "mcp/server/list": (params) => isRecord(params) &&
    (params.cursor === undefined || params.cursor === null || (typeof params.cursor === "string" && params.cursor.length <= 256)) &&
    (params.limit === undefined || params.limit === null || (typeof params.limit === "number" && Number.isInteger(params.limit) && params.limit >= 1 && params.limit <= 200)),
  "plugin/list": pluginListParams,
  "plugin/read": pluginReadParams,
  "plugin/install": pluginInstallParams,
  "plugin/update": pluginUpdateParams,
  "plugin/uninstall": pluginInstallIdParams,
  "plugin/enable": pluginInstallIdParams,
  "plugin/disable": pluginInstallIdParams,
  "profile/read": emptyParams,
  "skills/list": emptyParams,
  "skills/refresh": emptyParams,
  "skills/suggest": (params) =>
    isRecord(params) &&
    typeof params.query === "string" &&
    (params.limit === undefined || typeof params.limit === "number"),
  "skills/read": skillSelectorParams,
  "skills/resolve": (params) =>
    isRecord(params) &&
    typeof params.text === "string" &&
    Array.isArray(params.selected) &&
    params.selected.every(skillSelector),
  "turn/input": turnInputParams,
  "turn/interrupt": (params) => isRecord(params) && turnIdentity(params.identity),
  "turn/retry": (params) =>
    hasString(params, "sessionId") &&
    isRecord(params) &&
    agentUserInput(params.input) &&
    hasString(params, "clientMessageId") &&
    hasString(params, "retryOfMessageId"),
  "turn/start": (params) =>
    isRecord(params) &&
    hasString(params, "sessionId") &&
    agentUserInput(params.input) &&
    (params.clientRequestId === null || hasString(params, "clientRequestId")),
  "user/current": emptyParams,
  "workspace/info": emptyParams,
  "workspace/doc/list": emptyParams,
  "workspace/doc/read": (params) => hasString(params, "kind"),
  "workspace/doc/write": (params) => hasString(params, "kind") && hasString(params, "content"),
  "desktop/streamOpen":(params)=>hasString(params,"streamId")&&hasString(params,"ticket"),
  "desktop/streamSend":(params)=>hasString(params,"streamId")&&hasString(params,"dataBase64"),
  "desktop/streamClose":(params)=>hasString(params,"streamId"),
  "browser/operate": browserParams,
  "desktop/open": desktopOpenParams,
} satisfies Record<DesktopRendererMethod, ParamGuard>;

export function assertDesktopRendererInvoke(
  method: unknown,
  params: unknown,
  options?: unknown,
): {
  method: DesktopRendererMethod;
  actorCredentials?: AppProtocol.ActorCredentials | null;
} {
  if (typeof method !== "string" || !isDesktopRendererMethod(method)) {
    throw new Error(`Desktop renderer method is not allowed: ${String(method)}`);
  }

  const guard = DESKTOP_RENDERER_METHOD_GUARDS[method];
  if (!guard(params)) {
    throw new Error(`Invalid desktop renderer params for method: ${method}`);
  }
  return {
    method,
    actorCredentials: parseActorCredentials(options),
  };
}

export function isDesktopRendererMethod(method: string): method is DesktopRendererMethod {
  return method in DESKTOP_RENDERER_METHOD_GUARDS;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function hasString(params: unknown, key: string): boolean {
  return isRecord(params) && typeof params[key] === "string";
}

function hasArray(params: unknown, key: string): boolean {
  return isRecord(params) && Array.isArray(params[key]);
}

function parseActorCredentials(options: unknown): AppProtocol.ActorCredentials | null {
  if (options === undefined || options === null) {
    return null;
  }
  if (!isRecord(options)) {
    throw new Error("Invalid desktop renderer invoke options.");
  }
  const credentials = options.actorCredentials;
  if (credentials === undefined || credentials === null) {
    return null;
  }
  if (!isRecord(credentials) || !isRecord(credentials.credential)) {
    throw new Error("Invalid desktop renderer actor credentials.");
  }
  const credential = credentials.credential;
  if (credential.type !== "existing_session_token" || typeof credential.token !== "string") {
    throw new Error("Invalid desktop renderer actor credentials.");
  }
  return {
    credential: {
      type: "existing_session_token",
      token: credential.token,
    },
  };
}
