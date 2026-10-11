import type { TalosWebClient } from "./client";

/** Bind the selected Computer once; immutable Session APIs retain their own identity. */
export function bindComputerTarget(client: TalosWebClient, nodeId: string): TalosWebClient {
  const target = { nodeId };
  return { ...client,
    models: { list: (request) => client.models.list({ ...target, ...request }) },
    llm: { get: (request) => client.llm.get({ ...target, ...request }), update: (value, request) => client.llm.update(value, { ...target, ...request }) },
    config: { get: (path, request) => client.config.get(path, { ...target, ...request }), update: (value, request) => client.config.update(value, { ...target, ...request }) },
    sessions: { ...client.sessions, list: (request) => client.sessions.list({ projectId: null, limit: null, offset: null, ...target, ...request }) },
    projects: { ...client.projects, list: (request) => client.projects.list({ ...target, ...request }), create: (request) => client.projects.create({ ...target, ...request }) },
    fs: { directory: { read: (request) => client.fs.directory.read({ path: null, ...target, ...request }), create: (request) => client.fs.directory.create({ ...target, ...request }) } },
    memory: { list: (request) => client.memory.list({ ...target, ...request }), get: (request) => client.memory.get({ ...target, ...request }),
      put: (request) => client.memory.put({ ...target, ...request }), search: (request, options) => client.memory.search({ ...target, ...request }, options), getEntry: (request) => client.memory.getEntry({ ...target, ...request }) },
    workspaceDocs: { info: (request) => client.workspaceDocs.info({ ...target, ...request }),
      get: (kind, request) => client.workspaceDocs.get(kind, { ...target, ...request }), put: (kind, body, request) => client.workspaceDocs.put(kind, body, { ...target, ...request }) },
    skills: { list: (request) => client.skills.list({ include_disabled: false, ...target, ...request }), suggest: (request) => client.skills.suggest({ ...target, ...request }), read: (request) => client.skills.read({ ...target, ...request }) },
    mcp: { list: (request) => client.mcp.list({ ...target, ...request }), import: (request) => client.mcp.import({ ...target, ...request }),
      startAuthentication: (request) => client.mcp.startAuthentication({ ...target, ...request }), readAuthentication: (request) => client.mcp.readAuthentication({ ...target, ...request }), updateCredentials: (request) => client.mcp.updateCredentials({ ...target, ...request }) },
    plugin: { search: (request) => client.plugin.search({ ...target, ...request }), list: (request) => client.plugin.list({ ...target, ...request }), read: (request) => client.plugin.read({ ...target, ...request }),
      install: (request) => client.plugin.install({ ...target, ...request }), update: (request) => client.plugin.update({ ...target, ...request }), uninstall: (request) => client.plugin.uninstall({ ...target, ...request }),
      enable: (request) => client.plugin.enable({ ...target, ...request }), disable: (request) => client.plugin.disable({ ...target, ...request }), sources: { add: (request) => client.plugin.sources.add({ ...target, ...request }) } },
  };
}
