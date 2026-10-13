use super::*;
use crate::test_support::LocalSkillSourceFixture;
use std::sync::atomic::{AtomicBool, Ordering};
use talos_types::{PluginInstallConflictPolicy, PluginInstallRequest, PluginInstallTarget};

#[test]
fn failed_save_recovers_files_only_when_original_records_are_confirmed() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = LocalSkillSourceFixture::new(&temp);
    fixture.write_skill(
        "writer",
        "---\nname: writer\ndescription: Write.\n---\nOld version.\n",
    );
    let source = fixture.add_source(true);
    let installed = fixture
        .manager
        .install(PluginInstallRequest {
            node_id: None,
            target: PluginInstallTarget::Plugin {
                plugin_id: source.plugins[0].id.clone(),
                expected_source_commit: None,
            },
            enable: Some(true),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        })
        .unwrap();
    let previous = fixture.state.load_installs().unwrap();
    let mut candidate = previous.clone();
    candidate.installs[0].enabled = false;
    let active_file = installed.install.installed_path.join("SKILL.md");
    for persisted in [Some(&previous), Some(&candidate), None] {
        match persisted {
            Some(records) => fixture.state.save_installs(records).unwrap(),
            None => std::fs::write(fixture.state.installs_path(), "unreadable records").unwrap(),
        }
        std::fs::write(&active_file, "New version.").unwrap();
        let published = Arc::new(AtomicBool::new(false));
        let publication = published.clone();
        let error = fixture
            .state
            .with_exclusive_lock("fixture.commit", || {
                let failure = fixture.manager.confirm_failed_save(
                    &previous,
                    &candidate,
                    Some(Box::new(move || {
                        publication.store(true, Ordering::SeqCst);
                    })),
                    TalosError::internal_with_source(
                        "Fixture atomic write failed.",
                        std::io::Error::other("directory sync failed"),
                    ),
                );
                Ok(failure.rollback_if_uncommitted(|| {
                    std::fs::write(&active_file, "Old version.").unwrap();
                    Ok(())
                }))
            })
            .unwrap();
        assert!(error.is_internal());
        assert_eq!(
            published.load(Ordering::SeqCst),
            persisted == Some(&candidate)
        );
        assert_eq!(
            std::fs::read_to_string(&active_file).unwrap(),
            if persisted == Some(&previous) {
                "Old version."
            } else {
                "New version."
            }
        );
        if let Some(records) = persisted {
            assert_eq!(fixture.state.load_installs().unwrap(), *records);
        }
    }
}
