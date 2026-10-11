use crate::test_support::{manager_with_state_path, write_skill};
use talos_types::{MarketplaceSourceInput, PluginSourceAddRequest};

#[test]
fn git_source_refresh_produces_deterministic_plugin_ids() {
    let temp = tempfile::tempdir().expect("temp dir should exist");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir should be created");
    run_git(&repo, ["init"]);
    run_git(&repo, ["checkout", "-b", "main"]);
    run_git(&repo, ["config", "user.email", "talos-test@example.com"]);
    run_git(&repo, ["config", "user.name", "Talos Test"]);
    write_skill(
        &repo.join("skills"),
        "alpha",
        r#"---
name: alpha
description: Alpha git fixture skill.
---

Alpha skill.
"#,
    );
    run_git(&repo, ["add", "."]);
    run_git(&repo, ["commit", "-m", "add alpha skill"]);
    let url = format!("file://{}", repo.display());

    let first_added = manager_with_state_path(temp.path().join("state-one"))
        .add_source(PluginSourceAddRequest {
            node_id: None,
            source: MarketplaceSourceInput::Git {
                url: url.clone(),
                reference: "main".to_string(),
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        })
        .expect("git source should refresh");
    let second_added = manager_with_state_path(temp.path().join("state-two"))
        .add_source(PluginSourceAddRequest {
            node_id: None,
            source: MarketplaceSourceInput::Git {
                url,
                reference: "main".to_string(),
                sparse_paths: vec!["skills".to_string()],
            },
            refresh: true,
        })
        .expect("git source should refresh again");

    assert_eq!(first_added.plugins.len(), 1);
    assert_eq!(second_added.plugins.len(), 1);
    assert_eq!(first_added.plugins[0].id, second_added.plugins[0].id);
    assert_eq!(
        first_added.plugins[0].path_in_source.as_deref(),
        Some("skills/alpha")
    );
    assert!(first_added.plugins[0].source_commit.is_some());
}

fn run_git<const N: usize>(cwd: &std::path::Path, args: [&str; N]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
