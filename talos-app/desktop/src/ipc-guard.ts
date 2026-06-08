import type { AppProtocol } from "@talos/sdk-core";

type ParamGuard = (params: unknown) => boolean;
export type DesktopRendererMethod = Exclude<AppProtocol.ClientMethod, "initialize">;

const emptyParams = (params: unknown) => isRecord(params);
const sessionParams = (params: unknown) => hasString(params, "session_id");
const projectParams = (params: unknown) => hasString(params, "project_id");
const llmConfigWriteParams = (params: unknown) =>
  hasString(params, "path") &&
  isRecord(params) &&
  params.path === "llm" &&
  hasString(params, "value");
const authSessionStoreParams = (params: unknown) =>
  isRecord(params) &&
  isRecord(params.session) &&
  params.session.schema_version === 1 &&
  hasString(params.session, "access_token") &&
  hasString(params.session, "token_type") &&
  hasString(params.session, "cloud_api_base_url") &&
  hasString(params.session, "last_used_at_ms") &&
  isRecord(params.session.user) &&
  hasString(params.session.user, "user_id") &&
  hasString(params.session.user, "username");

const DESKTOP_RENDERER_METHOD_GUARDS = {
  "app.list": emptyParams,
  "auth.logout": emptyParams,
  "auth.session.clear": emptyParams,
  "auth.session.read": emptyParams,
  "auth.session.store": authSessionStoreParams,
  "config.read": emptyParams,
  "config.value.write": llmConfigWriteParams,
  "config.batch.write": (params) =>
    isRecord(params) &&
    Array.isArray(params.entries) &&
    params.entries.every(llmConfigWriteParams),
  "config.reload": emptyParams,
  "event.subscribe": sessionParams,
  "event.unsubscribe": (params) => hasString(params, "subscription_id"),
  "memory.file.list": emptyParams,
  "memory.file.read": (params) => hasString(params, "path"),
  "memory.file.write": (params) => hasString(params, "path") && hasString(params, "content"),
  "memory.search": (params) => hasString(params, "query"),
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
  "session.create": (params) => isRecord(params) && isRecord(params.context),
  "session.delete": sessionParams,
  "session.history": sessionParams,
  "session.list": emptyParams,
  "session.patch": sessionParams,
  "session.read": sessionParams,
  "session.recover": sessionParams,
  "skill.list": emptyParams,
  "tool.dynamic_result.submit": (params) =>
    hasString(params, "session_id") &&
    hasString(params, "call_id") &&
    hasString(params, "tool_name") &&
    hasString(params, "output") &&
    hasBoolean(params, "success"),
  "tool.list": emptyParams,
  "turn.interrupt": sessionParams,
  "turn.retry": (params) =>
    hasString(params, "session_id") &&
    hasArray(params, "input") &&
    hasString(params, "client_message_id") &&
    hasString(params, "retry_of_message_id"),
  "turn.start": (params) =>
    hasString(params, "session_id") &&
    hasArray(params, "input") &&
    hasString(params, "client_request_id"),
  "user.current": emptyParams,
  "workspace.doc.list": emptyParams,
  "workspace.doc.read": (params) => hasString(params, "kind"),
  "workspace.doc.write": (params) => hasString(params, "kind") && hasString(params, "content"),
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
