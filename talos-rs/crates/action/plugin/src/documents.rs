use std::fs;
use std::path::Path;
use talos_error::{TalosError, TalosResult};
use talos_types::{PluginDocumentMediaType, PluginDocumentRef, PluginDocumentRole, PluginKind};

const README_NAMES: [&str; 4] = ["README.md", "README", "readme.md", "Readme.md"];

pub(crate) fn readme_document_names() -> &'static [&'static str] {
    &README_NAMES
}

pub(crate) fn skill_document_ref(path: String) -> PluginDocumentRef {
    PluginDocumentRef {
        role: PluginDocumentRole::Primary,
        path,
        title: "SKILL.md".to_string(),
        media_type: PluginDocumentMediaType::Markdown,
    }
}

pub(crate) fn mcp_manifest_document_ref(path: String) -> PluginDocumentRef {
    PluginDocumentRef {
        role: PluginDocumentRole::Manifest,
        path,
        title: "talos-mcp.json".to_string(),
        media_type: PluginDocumentMediaType::Json,
    }
}

pub(crate) fn readme_document_refs(
    source_root: &Path,
    plugin_dir: &Path,
) -> TalosResult<Vec<PluginDocumentRef>> {
    let mut documents = Vec::new();
    for readme_name in README_NAMES {
        let path = plugin_dir.join(readme_name);
        if !is_regular_file(&path) {
            continue;
        }
        documents.push(PluginDocumentRef {
            role: PluginDocumentRole::Readme,
            path: relative_path(source_root, &path)?,
            title: readme_name.to_string(),
            media_type: document_media_type_for_name(readme_name),
        });
    }
    Ok(documents)
}

pub(crate) fn document_priority(kind: PluginKind, path: &Path) -> usize {
    let name = document_file_name(path).to_ascii_lowercase();
    match kind {
        PluginKind::McpServer if name.starts_with("readme") => 0,
        PluginKind::McpServer if name == "talos-mcp.json" => 1,
        PluginKind::Skill if name == "skill.md" => 0,
        PluginKind::Skill if name.starts_with("readme") => 1,
        _ => 2,
    }
}

pub(crate) fn document_title(path: &Path) -> String {
    let title = document_file_name(path);
    if title.is_empty() {
        "plugin document".to_string()
    } else {
        title
    }
}

pub(crate) fn document_role(kind: PluginKind, path: &Path) -> PluginDocumentRole {
    let name = document_file_name(path).to_ascii_lowercase();
    match kind {
        PluginKind::McpServer if name.starts_with("readme") => PluginDocumentRole::Readme,
        PluginKind::McpServer if name == "talos-mcp.json" => PluginDocumentRole::Manifest,
        PluginKind::Skill if name.starts_with("readme") => PluginDocumentRole::Readme,
        PluginKind::Skill if name == "skill.md" => PluginDocumentRole::Primary,
        _ => PluginDocumentRole::Primary,
    }
}

pub(crate) fn document_media_type(path: &Path) -> PluginDocumentMediaType {
    document_media_type_for_name(&document_file_name(path))
}

pub(crate) fn relative_path(root: &Path, path: &Path) -> TalosResult<String> {
    path.strip_prefix(root)
        .map_err(|err| {
            let diagnostic = format!(
                "plugin source path `{}` escaped root `{}`: {err}",
                path.display(),
                root.display()
            );
            TalosError::internal_with_source(
                "Plugin source path escaped root; technical details were withheld.",
                err,
            )
            .context(diagnostic)
        })
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn document_media_type_for_name(name: &str) -> PluginDocumentMediaType {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".md") || name == "readme" {
        return PluginDocumentMediaType::Markdown;
    }
    if name.ends_with(".json") {
        return PluginDocumentMediaType::Json;
    }
    PluginDocumentMediaType::Text
}

fn document_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}
