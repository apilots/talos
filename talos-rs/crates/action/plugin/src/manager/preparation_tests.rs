use super::*;
use crate::test_support::LocalSkillSourceFixture;

fn request(fixture: &LocalSkillSourceFixture) -> PluginInstallRequest {
    fixture.write_skill(
        "writer",
        "---\nname: writer\ndescription: Write.\n---\nOriginal.\n",
    );
    let source = fixture.add_source(true);
    PluginInstallRequest {
        node_id: None,
        target: PluginInstallTarget::Plugin {
            plugin_id: source.plugins[0].id.clone(),
            expected_source_commit: None,
        },
        enable: Some(true),
        conflict_policy: talos_types::PluginInstallConflictPolicy::Reject,
    }
}

#[test]
fn source_mutation_invalidates_preparation_without_touching_install_state() {
    let temporary = tempfile::tempdir().unwrap();
    let fixture = LocalSkillSourceFixture::new(&temporary);
    let request = request(&fixture);
    let prepared = fixture.manager.prepare_install(&request).unwrap();
    let staged = prepared
        .candidate
        .as_ref()
        .unwrap()
        .prepared_dir
        .clone()
        .unwrap();
    fixture.write_skill(
        "writer",
        "---\nname: writer\ndescription: Changed.\n---\nChanged.\n",
    );
    assert!(std::fs::read_to_string(staged.join("SKILL.md"))
        .unwrap()
        .contains("Original."));
    assert_eq!(
        fixture
            .manager
            .install_prepared(request, prepared)
            .unwrap_err()
            .error_code(),
        talos_error::ErrorCode::Conflict
    );
    assert!(fixture.state.load_installs().unwrap().installs.is_empty());
    assert!(!staged.exists());
}

#[test]
fn listing_cleans_abandoned_transactions_but_keeps_live_candidates() {
    let temporary = tempfile::tempdir().unwrap();
    let fixture = LocalSkillSourceFixture::new(&temporary);
    let request = request(&fixture);
    let abandoned = fixture.state.preparation_root().join("dead-worker");
    std::fs::create_dir_all(&abandoned).unwrap();
    std::fs::write(abandoned.join("candidate"), "abandoned content").unwrap();
    let transaction = fixture
        .state
        .transaction_temp_dir(&talos_types::PluginInstallId("abandoned".into()));
    std::fs::create_dir_all(&transaction).unwrap();
    let prepared = fixture.manager.prepare_install(&request).unwrap();
    let staged = prepared
        .candidate
        .as_ref()
        .unwrap()
        .prepared_dir
        .clone()
        .unwrap();
    fixture
        .manager
        .list_plugins(PluginListRequest {
            node_id: None,
            kind: None,
            status: None,
            source_id: None,
            include_disabled: true,
        })
        .unwrap();
    assert!(staged.join("SKILL.md").is_file());
    assert!(!abandoned.exists());
    assert!(!transaction.exists());
    fixture.manager.install_prepared(request, prepared).unwrap();
    assert!(!staged.exists());
    assert_eq!(fixture.state.load_installs().unwrap().installs.len(), 1);
}
