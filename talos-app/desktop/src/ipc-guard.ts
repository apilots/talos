import type { AppProtocol } from "@talos/sdk-core";

type ParamGuard = (params: unknown) => boolean;
export type DesktopRendererMethod = Exclude<AppProtocol.ClientMethod, "initialize">;

const emptyParams = (params: unknown) => isRecord(params);
const sessionParams = (params: unknown) => hasString(params, "session_id");
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
const pluginInstallParams = (params: unknown) =>
  isRecord(params) &&
  isRecord(params.target) &&
  params.target.kind === "plugin" &&
  typeof params.target.plugin_id === "string" &&
  (params.target.expected_source_commit === null ||
    params.target.expected_source_commit === undefined ||
    typeof params.target.expected_source_commit === "string") &&
  (params.enable === null || params.enable === undefined || typeof params.enable === "boolean") &&
  ["reject", "replace"].includes(String(params.conflict_policy));
const pluginInstallIdParams = (params: unknown) => isRecord(params) && hasString(params, "install_id");
const pluginUpdateParams = (params: unknown) =>
  isRecord(params) &&
  hasString(params, "install_id") &&
  (params.expected_source_commit === null ||
    params.expected_source_commit === undefined ||
    typeof params.expected_source_commit === "string") &&
  ["reject_local_changes", "replace_local_changes"].includes(String(params.conflict_policy));
const userActionSubmitParams = (params: unknown) =>
  hasString(params, "sessionId") &&
  hasString(params, "turnId") &&
  hasString(params, "actionId") &&
  isRecord(params) &&
  isRecord(params.response) &&
  ["question", "sandboxApproval", "dynamicToolResult"].includes(String(params.response.type));
const messageQueueSessionParams = (params: unknown) => hasString(params, "sessionId");
const messageQueueMessageParams = (params: unknown) =>
  messageQueueSessionParams(params) && hasString(params, "messageId");

const DESKTOP_RENDERER_METHOD_GUARDS = {
  "auth.login.cancel": authLoginSessionParams,
  "auth.login.start": authLoginStartParams,
  "auth.login.status": authLoginSessionParams,
  "auth.logout": emptyParams,
  "auth.session.clear": emptyParams,
  "auth.session.read": emptyParams,
  "config.get": configGetParams,
  "config.update": configWriteParams,
  "config.delete": (params) => hasString(params, "path"),
  "config.reload": emptyParams,
  "event.subscribe": sessionParams,
  "event.unsubscribe": (params) => hasString(params, "subscription_id"),
  "file/upload": fileUploadParams,
  "file/list": emptyParams,
  "file/read": fileIdParams,
  "file/update": fileIdParams,
  "file/delete": fileIdParams,
  "fs.directory.create": fsDirectoryCreateParams,
  "fs.directory.read": fsDirectoryReadParams,
  "memory.file.list": emptyParams,
  "memory.file.read": (params) => hasString(params, "date"),
  "memory.file.write": (params) => hasString(params, "date") && hasString(params, "content"),
  "memory/get": (params) => hasString(params, "memoryId"),
  "memory/search": (params) => hasString(params, "query"),
  "messageQueue/add": (params) => messageQueueSessionParams(params) && hasArray(params, "input"),
  "messageQueue/list": messageQueueSessionParams,
  "messageQueue/update": (params) => messageQueueMessageParams(params) && hasArray(params, "input"),
  "messageQueue/delete": messageQueueMessageParams,
  "messageQueue/reorder": (params) => messageQueueSessionParams(params) && hasArray(params, "messageIds"),
  "messageQueue/start": (params) =>
    messageQueueSessionParams(params) &&
    isRecord(params) &&
    (params.messageId === undefined || params.messageId === null || typeof params.messageId === "string"),
  "messageQueue/steer": (params) => messageQueueMessageParams(params) && hasString(params, "turnId"),
  "model.list": emptyParams,
  "project.create": (params) => hasString(params, "name") && hasString(params, "directory"),
  "project.delete": projectParams,
  "project.list": emptyParams,
  "project.patch": projectParams,
  "project.read": projectParams,
  "session.active_turn": sessionParams,
  "session.clear": sessionParams,
  "session.close": sessionParams,
  "session.compact": sessionParams,
  "session.delete": sessionParams,
  "session.history": sessionParams,
  "session.list": emptyParams,
  "session.patch": sessionParams,
  "session.get": sessionParams,
  "timeline.recover": sessionParams,
  "plugin.source.list": emptyParams,
  "plugin.source.add": pluginSourceAddParams,
  "plugin.source.refresh": pluginSourceIdParams,
  "plugin.source.remove": pluginSourceIdParams,
  "plugin.search": pluginSearchParams,
  "plugin.list": pluginListParams,
  "plugin.read": pluginReadParams,
  "plugin.install": pluginInstallParams,
  "plugin.update": pluginUpdateParams,
  "plugin.uninstall": pluginInstallIdParams,
  "plugin.enable": pluginInstallIdParams,
  "plugin.disable": pluginInstallIdParams,
  "profile/read": emptyParams,
  "skills.list": emptyParams,
  "skills.refresh": emptyParams,
  "skills.suggest": (params) =>
    isRecord(params) &&
    typeof params.query === "string" &&
    (params.limit === undefined || typeof params.limit === "number"),
  "skills.read": skillSelectorParams,
  "skills.resolve": (params) =>
    isRecord(params) &&
    typeof params.text === "string" &&
    Array.isArray(params.selected) &&
    params.selected.every(skillSelector),
  "tool.dynamic_result.submit": (params) =>
    hasString(params, "session_id") &&
    hasString(params, "call_id") &&
    hasString(params, "tool_name") &&
    hasString(params, "output") &&
    hasBoolean(params, "success"),
  "turn.interrupt": (params) => sessionParams(params) && hasString(params, "turn_id"),
  "turn.retry": (params) =>
    hasString(params, "session_id") &&
    hasArray(params, "input") &&
    hasString(params, "client_message_id") &&
    hasString(params, "retry_of_message_id"),
  "turn.start": (params) =>
    hasString(params, "session_id") &&
    hasArray(params, "input") &&
    hasString(params, "client_request_id"),
  "userAction/submit": userActionSubmitParams,
  "user.current": emptyParams,
  "workspace.doc.list": emptyParams,
  "workspace.doc.read": (params) => hasString(params, "kind"),
  "workspace.doc.write": (params) => hasString(params, "kind") && hasString(params, "content"),
  "workspace.info": emptyParams,
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

function hasBoolean(params: unknown, key: string): boolean {
  return isRecord(params) && typeof params[key] === "boolean";
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
