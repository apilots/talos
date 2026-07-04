use crate::documents::{readme_document_refs, relative_path, skill_document_ref};
use crate::source::plugin_id_for;
use std::path::Path;
use talos_error::TalosResult;
use talos_skills::{loader::parse_skill_dir, SkillLoadError, SkillRoot, SkillState, SkillSummary};
use talos_types::{
    MarketplaceSource, Plugin, PluginDefinition, PluginKind, PluginMetadata, PluginStatus,
    SkillManifestMetadata, SkillPlugin,
};

const MAX_MARKETPLACE_SKILL_MD_BYTES: usize = 512 * 1024;

pub(crate) fn plugin_from_skill_dir(
    source: &MarketplaceSource,
    source_root: &Path,
    scan_root: &Path,
    skill_dir: &Path,
) -> TalosResult<Result<Plugin, SkillLoadError>> {
    let skill_root = SkillRoot::configured(scan_root);
    let state = SkillState::default();
    let summary = match parse_skill_dir(
        skill_dir,
        &skill_root,
        &state,
        MAX_MARKETPLACE_SKILL_MD_BYTES,
    ) {
        Ok(summary) => summary,
        Err(error) => return Ok(Err(error)),
    };
    let path_in_source = relative_path(source_root, skill_dir)?;
    plugin_from_skill_summary(source, source_root, skill_dir, path_in_source, summary).map(Ok)
}

fn plugin_from_skill_summary(
    source: &MarketplaceSource,
    root: &Path,
    skill_dir: &Path,
    path_in_source: String,
    summary: SkillSummary,
) -> TalosResult<Plugin> {
    let skill_md_path = format!("{path_in_source}/SKILL.md");
    let skill_metadata = summary
        .metadata
        .as_ref()
        .map(|metadata| SkillManifestMetadata {
            display_name: metadata.display_name.clone(),
            icon: metadata.icon.clone(),
        });
    let metadata = PluginMetadata {
        name: summary.name,
        display_name: skill_metadata
            .as_ref()
            .and_then(|metadata| metadata.display_name.clone()),
        short_description: summary.short_description.clone(),
        description: summary.description,
        icon: skill_metadata.and_then(|metadata| metadata.icon),
        license: None,
    };
    let mut documents = vec![skill_document_ref(skill_md_path.clone())];
    documents.extend(readme_document_refs(root, skill_dir)?);
    Ok(Plugin {
        id: plugin_id_for(&source.id, PluginKind::Skill, &path_in_source),
        kind: PluginKind::Skill,
        metadata,
        documents,
        source_id: Some(source.id.clone()),
        path_in_source: Some(path_in_source),
        source_commit: source.last_checked_commit.clone(),
        status: PluginStatus::NotInstalled,
        definition: PluginDefinition::Skill(SkillPlugin { skill_md_path }),
        warnings: Vec::new(),
    })
}
