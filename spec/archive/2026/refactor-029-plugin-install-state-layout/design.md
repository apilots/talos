# Plugin Install State and Layout Refactor

Status: archived
Archived: 2026-07-14
Owner: agent
Created: 2026-06-24
Updated: 2026-06-24

## Background

The current plugin installer stores installed skills and MCP servers under opaque install-id directories:

```text
${TALOS_HOME}/plugins/installs/<install-id>/skills/<skill-name>
${TALOS_HOME}/plugins/installs/<install-id>/mcp/<server-name>
```

This layout was useful for a generic plugin transaction model, but it does not match the product model users see in the UI. Skills and MCP servers are first-class capability types, so their installed files should live in capability-specific directories.

The current `PluginInstall` state also persists a per-file manifest. The system only needs to detect whether the installed content changed as a whole, so persisting every file hash is unnecessary.

Compatibility with the current test installs is not required. This refactor should replace the old schema and old install layout directly.

## Goals

1. Store installed skill plugins under `${TALOS_HOME}/plugins/skills/<name>`.
2. Store installed MCP server plugins under `${TALOS_HOME}/plugins/mcp/<name>`.
3. Keep `PluginInstallId` as the API and state primary key, but do not use it as the user-visible install directory name.
4. Replace per-file persisted manifests with one directory-level content digest.
5. Keep plugin lifecycle state strongly typed in `talos-types`; do not manipulate raw JSON.
6. Derive runtime exposure from `kind + installed_path` instead of persisting `exposed_*` fields.
7. Remove old install-id directory logic and unused manifest code after the new model is in place.

## Non-Goals

- No migration from the old `plugin-installs.json` schema.
- No migration from `${TALOS_HOME}/plugins/installs/<install-id>`.
- No compatibility fallback for old install records.
- No UI redesign beyond adapting to the resulting API/state model.
- No MCP runtime management expansion in this refactor.

## Target Directory Layout

```text
${TALOS_HOME}/plugins/
  cache/
  skills/
    <skill-name>/
      SKILL.md
      ...
  mcp/
    <server-name>/
      talos-mcp.json
      ...
  plugin-installs.json
  plugin-state.lock
```

Transaction directories can still use the install id for uniqueness, but they should not become the final install location:

```text
${TALOS_HOME}/plugins/.tmp/<install-id>
${TALOS_HOME}/plugins/.backup/<install-id>
${TALOS_HOME}/plugins/.trash/<install-id>
```

## State Schema

`plugin-installs.json` must serialize strongly typed Rust objects from `talos-types`.

```rust
pub struct PluginInstallsFile {
    pub schema_version: u32,
    pub installs: Vec<PluginInstall>,
}

pub struct PluginInstall {
    pub id: PluginInstallId,
    pub plugin_id: PluginId,
    pub kind: PluginKind,
    pub name: String,
    pub installed_path: PathBuf,
    pub enabled: bool,
    pub source: PluginInstallSource,
    pub content_digest: ContentDigest,
    pub installed_at_ms: i64,
    pub updated_at_ms: i64,
}

pub struct ContentDigest {
    pub algorithm: ContentDigestAlgorithm,
    pub value: String,
    pub file_count: u64,
    pub total_size_bytes: u64,
}

pub enum ContentDigestAlgorithm {
    Sha256,
}
```

Remove these persisted fields and types:

```rust
PluginInstall::exposed_skill_root
PluginInstall::exposed_mcp_servers
PluginInstall::file_manifest
FileManifest
FileEntry
```

`enabled` belongs directly on `PluginInstall` because enable/disable is a plugin lifecycle state shared by skills and MCP servers.

## Install Path Rules

The final install path is derived from `PluginKind` and plugin name:

| Kind | Path |
| --- | --- |
| `Skill` | `${TALOS_HOME}/plugins/skills/<name>` |
| `McpServer` | `${TALOS_HOME}/plugins/mcp/<name>` |

`Package` and `Feature` remain non-installable until their lifecycle is explicitly designed.

## Content Digest

The content digest represents the full directory tree content. It is computed by recursively scanning files under the installed plugin directory.

The digest input must be stable:

1. Collect all regular files under the directory.
2. Sort files by normalized relative path.
3. For each file, feed these fields into the aggregate hash:
   - normalized relative path
   - file size
   - SHA-256 hash of file bytes
4. Persist only the aggregate digest plus file count and total size.

Any file content change, addition, deletion, rename, or move must change the aggregate digest.

## Install Flow

1. Resolve the install candidate by `plugin_id`.
2. Compute the final install path from `kind + name`.
3. Copy the candidate source directory into a temporary transaction directory.
4. Validate required files:
   - `Skill`: `SKILL.md`
   - `McpServer`: `talos-mcp.json`
5. Compute `content_digest` for the temporary directory.
6. Atomically rename the temporary directory to the final install path.
7. Write one `PluginInstall` record with:
   - `id`
   - `plugin_id`
   - `kind`
   - `name`
   - `installed_path`
   - `enabled`
   - `source`
   - `content_digest`
   - timestamps

## Update Detection

Marketplace refresh determines remote update state:

| Condition | Status |
| --- | --- |
| `plugin_id` is missing from refreshed sources | `SourceMissing` |
| latest source commit differs from `install.source.commit` | `UpdateAvailable` |
| latest source commit matches `install.source.commit` | `Installed` |

Local file changes do not determine whether a remote update exists.

## Local Change Detection

Before update, recompute the content digest for `installed_path`.

| Condition | Result |
| --- | --- |
| current digest matches `install.content_digest` | No local changes |
| current digest differs from `install.content_digest` | Local changes exist |

Update conflict policy determines behavior:

| Policy | Behavior |
| --- | --- |
| `RejectLocalChanges` | Fail the update |
| `ReplaceLocalChanges` | Replace installed content |

## Update Flow

1. Load the `PluginInstall` by `install_id`.
2. Resolve the latest candidate by `plugin_id`.
3. Recompute the current installed content digest.
4. Apply local-change conflict policy.
5. Copy the latest candidate into a temporary transaction directory.
6. Validate required files and compute the new digest.
7. Move the old `installed_path` into backup.
8. Rename the temporary directory to `installed_path`.
9. Update the install record:
   - `source`
   - `content_digest`
   - `updated_at_ms`
   - `name`, if the manifest name changed
10. Remove backup after the state write succeeds. Restore backup on failure.

## Uninstall Flow

1. Load the `PluginInstall` by `install_id`.
2. Move `installed_path` into a trash transaction directory.
3. Remove the install record from `plugin-installs.json`.
4. Delete trash after the state write succeeds. Restore trash on failure.

## Runtime Derivation

Do not persist runtime exposure fields.

Skill roots are derived from enabled skill installs:

```rust
if install.kind == PluginKind::Skill && install.enabled {
    SkillRoot::plugin(
        install.installed_path.clone(),
        install.plugin_id.0.clone(),
        install.id.0.clone(),
    )
}
```

MCP runtime descriptors are derived from enabled MCP installs:

```rust
if install.kind == PluginKind::McpServer && install.enabled {
    read install.installed_path.join("talos-mcp.json");
    resolve stdio cwd relative to install.installed_path;
    build McpServerRuntimeDescriptor;
}
```

## Impacted Areas

- `talos-types::plugin`
- `talos-plugin` state store
- `talos-plugin` installer transaction paths
- `talos-plugin` lifecycle manager
- `talos-plugin` MCP runtime descriptor derivation
- `talos-skills` plugin skill root integration
- HTTP and app-protocol generated schemas
- Web plugin management API types
- Plugin lifecycle tests

## Cleanup Requirements

Remove or replace:

- Final install layout under `plugins/installs/<install-id>`.
- Per-file manifest persistence.
- `FileManifest`.
- `FileEntry`.
- `manifest_for_dir` as a persisted-manifest helper.
- `install_dir_matches_record` logic based on per-file manifests.
- Persisted `exposed_skill_root`.
- Persisted `exposed_mcp_servers`.
- Tests that assert `installs/<install-id>/skills|mcp` final paths.

## Verification Strategy

Automated tests must cover:

- Skill install writes to `plugins/skills/<name>`.
- MCP install writes to `plugins/mcp/<name>`.
- `plugin-installs.json` stores `content_digest` and does not store per-file manifests.
- Local file modification changes the recomputed digest and rejects update when policy requires it.
- Update replaces content and refreshes `source` and `content_digest`.
- Uninstall removes state and installed directory.
- Enabled skill installs appear in skill roots.
- Enabled MCP installs produce runtime descriptors from `talos-mcp.json`.

Manual verification should install one skill and one MCP server from the Web UI and confirm the directory layout and management state.

## Risks

- Name-based install paths require clear conflict handling when two sources provide the same plugin name and kind.
- MCP manifests must be re-read from installed content, so manifest parsing errors become runtime derivation errors.
- Directory digest calculation must define exclusions deliberately; the first version should avoid implicit broad exclusions unless product-owned generated directories are introduced.
