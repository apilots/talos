use super::*;

#[test]
fn managed_profile_upgrade_preserves_saved_runtime_preferences() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    for profile in ["talos.yaml", "talos-prod.yaml", "talos-dev.yaml"] {
        let dir = tempfile::tempdir().unwrap();
        let paths = TalosPaths::from_home(dir.path().join("home"));
        let template_path = repo_root.join("conf").join(profile);
        let mut saved = read_yaml_file(&template_path).unwrap();
        assert_eq!(
            get_path_for_test(&saved, &["llm", "openai", "protocol"]).and_then(YamlValue::as_str),
            Some("responses"),
            "{profile}"
        );
        // The saved model catalog can differ from the shipped catalog.
        saved["llm"]["openai"]["models"] =
            serde_yaml::from_str("[{id: gpt-5.4-mini, context_window: 400000}]").unwrap();
        // Managed upgrades preserve the operator's previously saved Bailian catalog.
        saved["llm"]["bailian"]["models"]
            .as_sequence_mut()
            .unwrap()
            .retain(|model| !model["id"].as_str().unwrap().starts_with("qwen3.8-"));
        for (path, value) in [
            (vec!["llm", "provider"], "openai"),
            (vec!["llm", "model"], "gpt-5.4-mini"),
            (vec!["llm", "openai", "protocol"], "chat"),
            (vec!["sync", "enabled"], "false"),
            (vec!["agent", "identity"], "PERSONA.md"),
            (
                vec!["agent", "message_queue", "max_messages_per_session"],
                "23",
            ),
            (vec!["agent", "timeline_replay_batch_size"], "128"),
            (vec!["agent", "command_idempotency_ttl_hours"], "72"),
            (
                vec!["agent", "agents"],
                "[{id: researcher, name: Researcher, enabled: true}]",
            ),
            (
                vec!["agent", "kernel", "compaction", "model"],
                "deepseek-v4-flash",
            ),
            (
                vec![
                    "agent",
                    "kernel",
                    "tools",
                    "bash",
                    "default_timeout_ms",
                ],
                "90000",
            ),
            (
                vec!["agent", "kernel", "tools", "enable_dynamic_tools"],
                "true",
            ),
            (
                vec!["agent", "kernel", "tools", "dynamic", "response_timeout_ms"],
                "45000",
            ),
            (
                vec!["agent", "kernel", "tools", "mcp_runtime", "enable_tools"],
                "false",
            ),
            (vec!["logging", "timezone"], "UTC"),
            (vec!["logging", "target_levels"], "{talos_sync::runner: info, loro_internal::oplog::change_store::block_encode: debug}"),
            (vec!["llm", "ollama", "api_key"], "fixture-provider-key"),
            (vec!["llm", "deepseek", "http_timeouts"], "{connect_secs: 12, response_secs: 180, stream_idle_secs: 150, stream_total_secs: 1200}"),
            (vec!["llm", "openai", "http_timeouts"], "{connect_secs: 11, response_secs: 130, stream_idle_secs: 100, stream_total_secs: 950}"),
            (
                vec!["channels", "discord", "token"],
                "fixture-channel-token",
            ),
            (vec!["channels", "discord", "allow_from"], "[fixture-user]"),
        ] {
            set_path_for_test(&mut saved, &path, serde_yaml::from_str(value).unwrap());
        }
        let configured_path = [dir.path().join("first bin"), dir.path().join("second bin")];
        set_path_for_test(
            &mut saved,
            &["agent", "kernel", "tools", "environment", "path_prepend"],
            serde_yaml::to_value(configured_path).unwrap(),
        );
        // Old files omit defaults inside user-owned lists, maps and active optional groups.
        saved["llm"]["custom_providers"] = serde_yaml::from_str(
            "fixture-gateway: {name: Private gateway, protocol: chat, base_url: 'https://llm.example.invalid/v1', api_key: '${TALOS_LLM_FIXTURE_API_KEY}', http_timeouts: {response_secs: 180}, models: [{id: private-model, request_parameters: {custom: {enabled: true}}}]}",
        ).unwrap();
        saved["agent_node"]["runner"] = serde_yaml::from_str(
            "{node_id: fixture-node, control_plane_endpoint: 'https://control.example.invalid', credential_file: '/private/fixture-credential'}",
        ).unwrap();
        saved["channels"]["telegram"]["accounts"][0]
            .as_mapping_mut()
            .unwrap()
            .remove(YamlValue::String("token_file".into()));
        // A newly introduced nullable setting triggers normalization of a saved home.
        remove_path_for_test(&mut saved, &["sync", "trusted_ca_file"]);
        let mut expected: Config = serde_yaml::from_value(saved.clone()).unwrap();
        expected.llm.openai.protocol = Some(crate::LlmProtocol::Responses);
        expected.validate_runtime().unwrap();
        write_existing(&paths, &saved);

        let report = upgrade_default_config_from_template(&paths, &template_path).unwrap();
        assert!(report.changed, "{profile}");
        assert_eq!(report.defaulted_paths, ["llm.openai.protocol"], "{profile}");
        assert!(
            report
                .removed_paths
                .contains(&"logging.timezone".to_string()),
            "{profile}"
        );
        assert_eq!(
            read_yaml_file(report.backup_path.as_ref().unwrap()).unwrap(),
            saved
        );
        let upgraded = read_yaml_file(&paths.config_file).unwrap();
        assert_eq!(
            upgraded["llm"]["custom_providers"]["fixture-gateway"]["api_key"],
            "${TALOS_LLM_FIXTURE_API_KEY}"
        );
        for path in [
            "llm.openai.models[0].reasoning_replay",
            "llm.custom_providers.fixture-gateway.http_timeouts.connect_secs",
            "llm.custom_providers.fixture-gateway.models[0].cost",
            "agent_node.runner.trusted_ca_file",
            "channels.telegram.accounts[0].token_file",
        ] {
            assert!(
                report.added_paths.iter().any(|added| added == path),
                "{profile}: {path}"
            );
        }
        let actual: Config = serde_yaml::from_value(upgraded).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "{profile}"
        );
        assert!(
            !upgrade_default_config_from_template(&paths, &template_path)
                .unwrap()
                .changed
        );
    }
}

#[test]
fn managed_profiles_preserve_complete_valid_operator_config() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    for profile in ["talos.yaml", "talos-prod.yaml", "talos-dev.yaml"] {
        for variant in [
            "valid",
            "retired-mode",
            "invalid-compaction",
            "multiple-invalid-compaction",
            "ambiguous-memory",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let paths = TalosPaths::from_home(dir.path().join("home"));
            let template_path = repo_root.join("conf").join(profile);
            let mut config: Config =
                serde_yaml::from_value(read_yaml_file(&template_path).unwrap()).unwrap();
            config.workspaces = crate::WorkspaceConfig {
                default_workspace_id: "research".into(),
                definitions: vec![crate::WorkspaceDefinition::new(
                    "research",
                    "workspace-research",
                )],
            };
            config.agent.kernel.prompt.window.reserved_output_tokens =
                crate::ReservedOutputPolicy::Ratio {
                    numerator: 1,
                    denominator: 8,
                };
            config.storage.files.preview_base_url = Some("https://preview.example.invalid".into());
            config.memory.file_list_default_limit = 100;
            config.memory.file_list_max_limit = 200;
            config.agent.kernel.output.max_tokens = 100000;
            config.agent.kernel.output.upper_limit = 200000;
            config.agent.kernel.compaction.trigger_ratio_percent = 95;
            config.agent.kernel.compaction.hard_ratio_percent = 99;
            config.agent.kernel.compaction.recent_messages_max_chars = 10000;
            config
                .agent
                .kernel
                .compaction
                .compacted_recent_messages_max_chars = 9000;
            config.agent.kernel.compaction.summary_max_chars = 5000;
            config.agent.kernel.compaction.compacted_summary_max_chars = 4000;
            config.agent.kernel.compaction.tool_exchange_max_chars = 5000;
            config
                .agent
                .kernel
                .compaction
                .compacted_tool_exchange_max_chars = 4000;
            config.agent.kernel.compaction.retrieved_memory_max_chars = 5000;
            config
                .agent
                .kernel
                .compaction
                .compacted_retrieved_memory_max_chars = 4000;
            config.agent.kernel.tools.bash.output_log.max_file_bytes = 5368709120;
            config
                .agent
                .kernel
                .tools
                .bash
                .output_log
                .max_workspace_bytes = 10737418240;
            config.storage.files.limits.max_metadata_value_bytes = 16384;
            config.storage.files.limits.max_metadata_bytes = 32768;
            config.agent.subagent_admission.max_depth = std::num::NonZeroUsize::MIN;
            config.agent.subagent_admission.max_active_turns = std::num::NonZeroU32::MIN;
            config.validate_runtime().unwrap();

            let mut saved = serde_yaml::to_value(config).unwrap();
            // Runtime-only paths are not writable operator configuration.
            for path in [
                vec!["logging", "file"],
                vec!["storage", "connection_string"],
                vec!["agent", "kernel", "timezone"],
            ] {
                remove_path_for_test(&mut saved, &path);
            }
            remove_path_for_test(&mut saved, &["sync", "trusted_ca_file"]);
            let expected: Config = serde_yaml::from_value(saved.clone()).unwrap();
            expected.validate_runtime().unwrap();
            if variant == "retired-mode" {
                set_path_for_test(&mut saved, &["mode"], YamlValue::String("local".into()));
            }
            if matches!(
                variant,
                "invalid-compaction" | "multiple-invalid-compaction"
            ) {
                set_path_for_test(
                    &mut saved,
                    &["agent", "kernel", "compaction", "recent_messages_keep"],
                    YamlValue::Number(0.into()),
                );
            }
            if variant == "multiple-invalid-compaction" {
                set_path_for_test(
                    &mut saved,
                    &[
                        "agent",
                        "kernel",
                        "compaction",
                        "compacted_recent_messages_keep",
                    ],
                    YamlValue::Number(0.into()),
                );
            }
            if variant == "ambiguous-memory" {
                set_path_for_test(
                    &mut saved,
                    &["memory", "file_list_default_limit"],
                    YamlValue::Number(80.into()),
                );
                set_path_for_test(
                    &mut saved,
                    &["memory", "file_list_max_limit"],
                    YamlValue::Number(60.into()),
                );
            }
            write_existing(&paths, &saved);
            let original = std::fs::read(&paths.config_file).unwrap();

            let report = match upgrade_default_config_from_template(&paths, &template_path) {
                Err(error)
                    if matches!(variant, "multiple-invalid-compaction" | "ambiguous-memory") =>
                {
                    assert_eq!(error.error_code(), talos_error::ErrorCode::InvalidRequest);
                    assert_eq!(std::fs::read(&paths.config_file).unwrap(), original);
                    let backups = std::fs::read_dir(&paths.conf_dir)
                        .unwrap()
                        .filter_map(Result::ok)
                        .filter(|entry| entry.file_name().to_string_lossy().contains(".bak."))
                        .count();
                    assert_eq!(backups, 0);
                    continue;
                }
                result => result.unwrap(),
            };
            assert_ne!(
                variant, "ambiguous-memory",
                "an ambiguous repair must not guess operator intent"
            );
            assert!(report.changed, "{profile}, variant={variant}");
            let actual: Config =
                serde_yaml::from_value(read_yaml_file(&paths.config_file).unwrap()).unwrap();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(expected).unwrap(),
                "{profile}, variant={variant}"
            );
            let expected_removed: Vec<String> = if variant == "retired-mode" {
                vec!["mode".into()]
            } else {
                Vec::new()
            };
            assert_eq!(report.removed_paths, expected_removed, "{profile}");
            let mut expected_defaulted: Vec<String> = match variant {
                "invalid-compaction" => vec!["agent.kernel.compaction.recent_messages_keep".into()],
                "multiple-invalid-compaction" => vec![
                    "agent.kernel.compaction.recent_messages_keep".into(),
                    "agent.kernel.compaction.compacted_recent_messages_keep".into(),
                ],
                _ => Vec::new(),
            };
            expected_defaulted.sort();
            let mut actual_defaulted = report.defaulted_paths.clone();
            actual_defaulted.sort();
            assert_eq!(
                actual_defaulted, expected_defaulted,
                "{profile}, variant={variant}"
            );
            assert!(
                report.added_paths.contains(&"sync.trusted_ca_file".into()),
                "{profile}"
            );
            assert_eq!(
                read_yaml_file(report.backup_path.as_ref().unwrap()).unwrap(),
                saved
            );
            assert!(
                !upgrade_default_config_from_template(&paths, &template_path)
                    .unwrap()
                    .changed
            );
        }
    }
}
