use super::*;
use talos_config::PluginsRuntimeConfig;
use talos_plugin::{PluginManager, PluginStateStore};

fn test_service(
    temp: &tempfile::TempDir,
    config: SkillsRuntimeConfig,
    plugins: PluginsRuntimeConfig,
) -> SkillsService {
    let manager = PluginManager::with_state_store(
        plugins.clone(),
        PluginStateStore::new(temp.path().join("plugin-state")),
    );
    SkillsService::new(config, PluginManagerService::with_manager(manager))
}

#[test]
fn skills_service_lists_suggests_reads_and_resolves_from_configured_roots() {
    let temp = tempfile::tempdir().expect("temp dir should be created");
    let external_root = temp.path().join("external-skills");
    let skill_root = external_root.join("review");
    talos_fs::create_directory(&skill_root, true).expect("skill root should be created");
    talos_fs::write_bytes(
        &skill_root.join("SKILL.md"),
        b"---\nname: review\ndescription: Review code changes.\n---\n\n# Review\n\nUse evidence.",
    )
    .expect("skill should be written");

    let plugins = PluginsRuntimeConfig {
        sources: Vec::new(),
        roots: vec![talos_config::PluginRootConfig {
            kind: talos_config::PluginRootKind::Skill,
            path: external_root.display().to_string(),
            read_only: true,
        }],
    };
    let service = test_service(
        &temp,
        SkillsRuntimeConfig {
            max_skill_md_bytes: 64 * 1024,
            max_available_summary_bytes: 8 * 1024,
        },
        plugins,
    );

    let listed = service.list(false).expect("skills should list");
    assert_eq!(listed.skills.len(), 1);
    assert_eq!(listed.skills[0].name, "review");

    let suggestions = service
        .suggest("rev", 10)
        .expect("skills should suggest by query");
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].path, skill_root.join("SKILL.md"));

    let document = service
        .read_document(&SkillSelector::by_path(skill_root.join("SKILL.md")))
        .expect("skill document should read");
    assert!(document.contents.contains("Use evidence."));

    let (resolved, warnings) = service
        .resolve(
            "Use $review",
            &[SkillSelector::selection(
                "review",
                skill_root.join("SKILL.md"),
            )],
        )
        .expect("skill should resolve");
    assert!(warnings.is_empty());
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, "review");
}
