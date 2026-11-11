# Plugin lifecycle and local packages

Status: verified

## Background and goals

The CLI currently exposes generic installation IDs and a standalone MCP JSON import command. Expose the existing Plugin lifecycle by type and name. Use one Marketplace source, scanner, and installation pipeline for Git, local directories, and local package files.

## Commands

- `talos plugin marketplace add|list|refresh|remove` operates on Marketplace sources.
- `talos plugin skill add <name[@marketplace]>` and `talos plugin mcp add <name[@marketplace]>` select a catalog entry by exact type, name, and optional source. Ambiguous matches report a conflict. The source name shown by the CLI or its existing source ID identifies a Marketplace source.
- `talos plugin skill|mcp list` shows installations of that type. `remove`, `enable`, `disable`, and `update` take the installed name on the selected Worker. Internal installation IDs remain the mutation keys.
- `talos plugin skill|mcp add --file <path>` imports a local package. A file path is resolved by the target Worker. For an embedded Worker the CLI resolves a relative path against its working directory. An explicit `--node` uses the named Worker's filesystem. Local replacement takes a new package and `--replace`.

The existing CLI commands retain their behavior for existing scripts. All commands use the existing Gateway and Plugin services.

## Formats and ownership

- A Skill directory contains a `SKILL.md` with `name` and `description`. Bundled resources live within the directory.
- An MCP directory contains `mcp.json` with the existing single-server `name` and `transport` shape. The Worker accepts `talos-mcp.json` for existing sources and installations. Both filenames resolve through one parser. The canonical name wins when both are present.
- A ZIP contains one top-level plugin directory. An MCP with no bundled files can also use its single JSON manifest directly. The existing manual MCP JSON import remains available for its existing input dialect.
- A local package is a typed Marketplace source. The ZIP importer rejects escaping paths and symbolic links. The author supplies bundled files and the runtime owns external commands, credentials, and connection health. Talos maintains installation state. There is no separate `plugin.json` contract.

A standalone MCP manifest can define a remote server with this shape.

```json
{
  "name": "example-tools",
  "transport": {
    "type": "streamable_http",
    "url": "https://example.com/mcp"
  }
}
```

For a stdio server with bundled resources, put `mcp.json` and those resources together in the ZIP's plugin directory. The installed directory is the default working directory for stdio.

The Worker imports a contained snapshot into the existing Marketplace cache and installs the selected candidate through the existing transaction. It records the source identity in the managed installation. Importing identical bytes reuses the registered snapshot. Source refresh and update cannot fetch changes for a local package. The user supplies a replacement explicitly. Built-in Browser identity and privileges remain governed by the existing official-source checks.

## Verification

Exercise Marketplace discovery, Skill and MCP ZIP installation, direct canonical MCP JSON, replacement, filtering, enablement, removal, and old manifest reads. Verify archive containment and that a failed import does not change installed state. Run scoped Rust checks, generated-protocol validation, and a local CLI smoke. Actual results belong under `output/`.

## Boundaries

General dependency installation, a second Plugin registry, automatic updates for local packages, and transfer of files from the CLI host to a different Worker are outside this spec.
