import type { TalosWebClient } from "./client";

/** Bind the selected Computer once; immutable Session APIs retain their own identity. */
export function bindComputerTarget(
  client: TalosWebClient,
  nodeId: string,
  resolve?: (nodeId: string) => Promise<TalosWebClient>,
): TalosWebClient {
  const target = { nodeId };
  const owner = (request?: { nodeId?: string | null }) =>
    resolve ? resolve(request?.nodeId ?? nodeId) : client;
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
      list: async request => (await owner(request)).sessions.list({ projectId: null, limit: null, offset: null, ...target, ...request }),
    },
    projects: {
      list: async request => (await owner(request)).projects.list({ ...target, ...request }),
      create: async request => (await owner(request)).projects.create({ ...target, ...request }),
      patch: async request => (await owner()).projects.patch(request),
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
