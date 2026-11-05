use super::*;
use crate::PluginStateStore;
use talos_config::{PluginsRuntimeConfig, SkillsRuntimeConfig};
use talos_skills::{read_document, refresh, system_automation_selector};

#[test]
fn same_plugin_owner_provisions_two_read_only_system_skills() {
    let temp = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temp.path());
    let manager = PluginManager::with_state_store(PluginsRuntimeConfig::default(), state.clone());
    let roots = manager
        .skill_roots(&SkillsRuntimeConfig::default())
        .unwrap();
    assert_eq!(
        roots,
        manager
            .skill_roots(&SkillsRuntimeConfig::default())
            .unwrap()
    );
    assert_eq!(roots.state_path, state.skills_state_path());
    let system = roots
        .roots
        .iter()
        .find(|root| root.scope == SkillScope::System)
        .unwrap();
    assert_eq!(system.path, state.system_skills_dir());
    assert!(system.read_only);
    let skills = refresh(&roots).unwrap();
    assert_eq!(
        skills
            .skills
            .iter()
            .filter(|skill| skill.scope == SkillScope::System)
            .count(),
        2
    );
    for skill in builtin_automation_skills() {
        let selected = system_automation_selector(&roots, skill.mode).unwrap();
        let document = read_document(&roots, &selected).unwrap();
        assert_eq!(document.contents, skill.contents);
        assert!(document.summary.read_only);
        assert!(document.summary.source_install.is_none());
    }
}

#[test]
fn repairs_changed_builtin_content_from_embedded_asset() {
    let temp = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temp.path());
    let manager = PluginManager::with_state_store(PluginsRuntimeConfig::default(), state.clone());
    manager
        .skill_roots(&SkillsRuntimeConfig::default())
        .unwrap();
    let path = state.system_skills_dir().join("goal/SKILL.md");
    std::fs::write(&path, [0xff, 0, 0xfe]).unwrap();
    manager
        .skill_roots(&SkillsRuntimeConfig::default())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        builtin_automation_skills()[0].contents
    );
}

#[test]
fn concurrent_catalogue_queries_use_the_original_state_lock() {
    let temp = tempfile::tempdir().unwrap();
    let manager = PluginManager::with_state_store(
        PluginsRuntimeConfig::default(),
        PluginStateStore::new(temp.path()),
    );
    let threads = (0..4)
        .map(|_| {
            let manager = manager.clone();
            std::thread::spawn(move || {
                manager
                    .skill_roots(&SkillsRuntimeConfig::default())
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let roots = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert!(roots.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(refresh(&roots[0]).unwrap().errors.is_empty());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_system_root_without_writing_to_its_target() {
    let temp = tempfile::tempdir().unwrap();
    let state = PluginStateStore::new(temp.path().join("state"));
    std::fs::create_dir_all(state.skill_installs_dir()).unwrap();
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, state.system_skills_dir()).unwrap();
    let manager = PluginManager::with_state_store(PluginsRuntimeConfig::default(), state);
    assert!(manager
        .skill_roots(&SkillsRuntimeConfig::default())
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}
