use super::*;
use talos_types::*;

#[tokio::test]
async fn rejected_install_and_update_release_worker_preparations() {
    for updating in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let skill = root.path().join("catalog/skills/writer");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: writer\ndescription: Write.\n---\nWrite.\n",
        )
        .unwrap();
        let plugins = crate::PluginManagerService::with_manager(
            talos_plugin::PluginManager::with_state_store(
                talos_config::PluginsRuntimeConfig {
                    sources: vec![],
                    roots: vec![],
                },
                talos_plugin::PluginStateStore::new(root.path().join("plugins")),
            ),
        );
        let source = plugins
            .add_source(PluginSourceAddRequest {
                node_id: None,
                source: MarketplaceSourceInput::LocalFolder {
                    path: root.path().join("catalog"),
                    sparse_paths: vec![],
                },
                refresh: true,
            })
            .unwrap();
        let install = PluginInstallRequest {
            node_id: None,
            target: PluginInstallTarget::Plugin {
                plugin_id: source.plugins[0].id.clone(),
                expected_source_commit: None,
            },
            enable: Some(false),
            conflict_policy: PluginInstallConflictPolicy::Reject,
        };
        let update = if updating {
            Some(PluginUpdateRequest {
                node_id: None,
                install_id: plugins.install(install.clone()).await.unwrap().install.id,
                expected_source_commit: None,
                conflict_policy: PluginUpdateConflictPolicy::RejectLocalChanges,
            })
        } else {
            None
        };
        let config = configured_test_config();
        let environment = Arc::new(
            crate::WorkerEnvironmentService::new(
                Arc::new(std::sync::RwLock::new(config.clone())),
                Some(root.path().to_path_buf()),
            )
            .with_plugin_manager(plugins),
        );
        let (service, store) = test_application_with_environment(
            Arc::new(TestAgentExecution {
                mode: TestAgentExecutionMode::Accepted,
            }),
            Arc::new(crate::test_support::NoopAgentCoordinator),
            config,
            Some(environment),
        )
        .await;
        let actor = Actor::regular("member-user");
        service
            .create_session(
                &actor,
                create_session_params("blocked", request_source(), None),
            )
            .await
            .unwrap();
        let identity = AgentTurnRef::new("blocked", "running");
        let message = crate::turn::completed_agent_user_input(
            &identity.turn_id,
            "input",
            crate::test_support::test_user_input("running"),
            1,
        )
        .unwrap();
        crate::test_support::commit_test_running_turn(store.as_ref(), identity, message).await;
        for _ in 0..4 {
            let result = match &update {
                Some(request) => service
                    .update_plugin(&actor, request.clone())
                    .await
                    .map(|_| ()),
                None => service
                    .install_plugin(&actor, install.clone())
                    .await
                    .map(|_| ()),
            };
            assert_eq!(
                result.unwrap_err().error_code(),
                talos_error::ErrorCode::Conflict
            );
        }
        // Remove only the synthetic blocking Session; management must work immediately.
        store.delete_session_record("blocked").await.unwrap();
        match update {
            Some(request) => {
                service.update_plugin(&actor, request).await.unwrap();
            }
            None => {
                service.install_plugin(&actor, install).await.unwrap();
            }
        }
    }
}
