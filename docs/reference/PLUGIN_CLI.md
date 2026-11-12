# Plugin CLI

Use `talos plugin skill` and `talos plugin mcp` to manage installed plugins by name. The CLI uses the embedded Worker by default. Add `--node <computer-id>` to target a different Worker.

## Command help

```bash
talos plugin --help
talos plugin skill --help
talos plugin mcp --help
talos plugin marketplace --help
talos plugin mcp add --help
```

## Install and manage by name

Start with a configured Marketplace source. Use the exact name declared by the plugin. Add `@marketplace` when the name matches more than one source. Find the Marketplace's display name and source ID with `talos plugin marketplace list`. Use the source ID if display names collide.

```bash
talos plugin skill add review-checklist@team-tools
talos plugin mcp add issue-tools@team-tools
talos plugin skill list
talos plugin mcp list
```

Both types support `enable`, `disable`, `update`, and `remove`. Each takes the installed name. The lists include disabled installations. For Git and directory sources, `update` refreshes the source and refuses to overwrite local edits.

```bash
talos plugin mcp disable issue-tools
talos plugin mcp enable issue-tools
talos plugin mcp update issue-tools
talos plugin mcp remove issue-tools
```

## Install a local package

Put one plugin directory at the root of the ZIP. A Skill uses `SKILL.md` with `name` and `description` in its frontmatter. An MCP plugin uses `mcp.json` with a single server's `name` and `transport`. Put any bundled files beside the manifest or in subdirectories. An MCP plugin without bundled files can use a standalone `mcp.json`.

```text
review-checklist.zip             issue-tools.zip
  review-checklist/              issue-tools/
    SKILL.md                       mcp.json
    references/                    server.py
```

A standalone remote MCP configuration can use this shape. Replace the URL with your MCP endpoint.

```json
{
  "name": "issue-tools",
  "transport": {
    "type": "streamable_http",
    "url": "https://example.com/mcp"
  }
}
```

```bash
talos plugin skill add --file ./review-checklist.zip
talos plugin mcp add --file ./issue-tools.zip
talos plugin mcp add --file ./mcp.json
talos plugin skill add --file ./review-checklist-v2.zip --replace
```

The ZIP and standalone JSON commands are alternative ways to install the same MCP example. Talos validates the package type before registering a local Marketplace source. To update an installed package, supply the new file with `--replace`. The replacement retains the current enabled or disabled state.

The selected Worker resolves the path. For a remote Worker, pass an absolute path to a file already on that Worker.

Existing `talos-mcp.json` manifests remain readable. Existing scripts with a one-entry `mcpServers` JSON document can continue using `talos plugin import-mcp <file>`.

## Manage Marketplace sources

Add a Git repository or a directory on the selected Worker. For Git sources, `--reference` selects a revision and defaults to `main`. The following examples use a placeholder URL and directory.

```bash
talos plugin marketplace add https://example.com/team/team-tools.git --reference main
talos plugin marketplace add ./local-catalog
talos plugin marketplace list
talos plugin marketplace refresh team-tools
talos plugin marketplace remove team-tools
```

Use `refresh` to rescan a Git or directory source. `remove` unregisters the source. Use `talos plugin skill remove` or `talos plugin mcp remove` to uninstall a plugin.
