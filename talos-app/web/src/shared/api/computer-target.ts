import type { TalosWebClient } from "./client";

/** Display the addressed product instead of its private embedded execution alias. */
export function productComputerRecord<T extends { nodeId: string }>(record: T, computerId?: string): T {
  return record.nodeId === "embedded" && computerId && computerId !== "embedded"
    ? { ...record, nodeId: computerId } : record;
}

/** Bind the selected Computer once; immutable Session APIs retain their own identity. */
export function bindComputerTarget(
  client: TalosWebClient,
  nodeId: string,
  resolve?: (nodeId: string) => Promise<TalosWebClient>,
): TalosWebClient {
  client = nodeId === "embedded" ? client : client.forComputer?.(nodeId) ?? client;
  const target = { nodeId };
  const owner = (request?: { nodeId?: string | null }) =>
    resolve ? resolve(request?.nodeId ?? nodeId).then(owner => owner.forComputer?.(request?.nodeId ?? nodeId) ?? owner) : client;
  return {
    ...client,
    models: { list: async request => (await owner(request)).models.list({ ...target, ...request }) },
    llm: {
      get: async request => (await owner(request)).llm.get({ ...target, ...request }),
      update: async (value, request) => (await owner(request)).llm.update(value, { ...target, ...request }),
    },
    config: {
      get: async (path, request) => (await owner(request)).config.get(path, { ...target, ...request }),
      update: async (value, request) => (await owner(request)).config.update(value, { ...target, ...request }),
    },
    sessions: {
      ...client.sessions,
      get: async request => {
        const response = await client.sessions.get(request);
        return { ...response, session: productComputerRecord(response.session, nodeId) };
      },
      list: async request => {
        const response = await (await owner(request)).sessions.list({ projectId: null, limit: null, offset: null, ...target, ...request });
        return { ...response, sessions: response.sessions.map(session => productComputerRecord(session, request?.nodeId ?? nodeId)) };
      },
      patch: async request => {
        const response = await client.sessions.patch(request);
        return { ...response, session: productComputerRecord(response.session, nodeId) };
      },
      compact: async request => {
        const response = await client.sessions.compact(request);
        return { ...response, session: productComputerRecord(response.session, nodeId) };
      },
      clear: async request => {
        const response = await client.sessions.clear(request);
        return { ...response, session: productComputerRecord(response.session, nodeId) };
      },
    },
    projects: {
      list: async request => {
        const response = await (await owner(request)).projects.list({ ...target, ...request });
        return { ...response, projects: response.projects.map(project => productComputerRecord(project, request?.nodeId ?? nodeId)) };
      },
      create: async request => {
        const response = await (await owner(request)).projects.create({ ...target, ...request });
        return { ...response, project: productComputerRecord(response.project, request.nodeId ?? nodeId) };
      },
      patch: async request => {
        const response = await (await owner()).projects.patch(request);
        return { ...response, project: productComputerRecord(response.project, nodeId) };
      },
      delete: async request => (await owner()).projects.delete(request),
    },
    fs: { directory: {
      read: async request => (await owner(request)).fs.directory.read({ path: null, ...target, ...request }),
      create: async request => (await owner(request)).fs.directory.create({ ...target, ...request }),
    } },
    memory: {
      list: async request => (await owner(request)).memory.list({ ...target, ...request }),
      get: async request => (await owner(request)).memory.get({ ...target, ...request }),
      put: async request => (await owner(request)).memory.put({ ...target, ...request }),
      search: async (request, options) => (await owner(request)).memory.search({ ...target, ...request }, options),
      getEntry: async request => (await owner(request)).memory.getEntry({ ...target, ...request }),
    },
    workspaceDocs: {
      info: async request => (await owner(request)).workspaceDocs.info({ ...target, ...request }),
      get: async (kind, request) => (await owner(request)).workspaceDocs.get(kind, { ...target, ...request }),
      put: async (kind, body, request) => (await owner(request)).workspaceDocs.put(kind, body, { ...target, ...request }),
    },
    skills: {
      list: async request => (await owner(request)).skills.list({ include_disabled: false, ...target, ...request }),
      suggest: async request => (await owner(request)).skills.suggest({ ...target, ...request }),
      read: async request => (await owner(request)).skills.read({ ...target, ...request }),
    },
    desktop: { ...client.desktop, open: async request => (await owner()).desktop.open({ ...request, nodeId }) },
    browser: { operate: async request => (await owner()).browser.operate({ ...request, nodeId }) },
    mcp: {
      list: async request => (await owner(request)).mcp.list({ ...target, ...request }),
      import: async request => (await owner(request)).mcp.import({ ...target, ...request }),
      startAuthentication: async request => (await owner(request)).mcp.startAuthentication({ ...target, ...request }),
      readAuthentication: async request => (await owner(request)).mcp.readAuthentication({ ...target, ...request }),
      updateCredentials: async request => (await owner(request)).mcp.updateCredentials({ ...target, ...request }),
    },
    plugin: {
      search: async request => (await owner(request)).plugin.search({ ...target, ...request }),
      list: async request => (await owner(request)).plugin.list({ ...target, ...request }),
      read: async request => (await owner(request)).plugin.read({ ...target, ...request }),
      install: async request => (await owner(request)).plugin.install({ ...target, ...request }),
      update: async request => (await owner(request)).plugin.update({ ...target, ...request }),
      uninstall: async request => (await owner(request)).plugin.uninstall({ ...target, ...request }),
      enable: async request => (await owner(request)).plugin.enable({ ...target, ...request }),
      disable: async request => (await owner(request)).plugin.disable({ ...target, ...request }),
      sources: { add: async request => (await owner(request)).plugin.sources.add({ ...target, ...request }) },
    },
  };
}

/** Defer ownership lookup without restricting routing to only a subset of APIs. */
export function deferProductClient(local: TalosWebClient, resolve: () => Promise<TalosWebClient>): TalosWebClient {
  const productNamespaces = new Set(["trajectory", "automation", "builtinJobs", "profile", "llm", "config", "models", "sessions", "messageQueue", "projects", "files", "fs", "memory", "workspaceDocs", "skills", "desktop", "browser", "mcp", "plugin", "turns"]);
  const member = (source: unknown, path: PropertyKey[]): unknown => path.reduce((value, key) => Reflect.get(value as object, key), source);
  const namespace = (path: PropertyKey[]): object => new Proxy((member(local, path) ?? {}) as object, {
    get: (_, key) => {
      const next = [...path, key];
      const value = member(local, next);
      // Prepared preview descriptors already carry presentation identity.
      if ((path[0] === "desktop" && key === "connect") ||
        (path[0] === "files" && path[1] === "preview" && (key === "resourceUrl" || key === "resolvePath"))) return value;
      return typeof value === "function"
        ? async (...args: unknown[]) => {
            const client = await resolve();
            const parent = member(client, path);
            const callable: unknown = Reflect.get(parent as object, key);
            if (typeof callable !== "function") throw new Error("Product method is unavailable.");
            return Reflect.apply(callable as (this: unknown, ...args: unknown[]) => unknown, parent, args);
          }
        : namespace(next);
    },
  });
  return new Proxy(local, {
    get: (source, key, receiver): unknown => {
      const value: unknown = typeof key === "string" && productNamespaces.has(key)
        ? namespace([key]) : Reflect.get(source, key, receiver);
      return value;
    },
  });
}
