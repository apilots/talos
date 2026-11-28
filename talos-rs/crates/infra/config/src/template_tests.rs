use super::*;

#[test]
fn test_product_configs_include_configured_provider_models() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    let config_paths = ["talos.yaml", "talos-dev.yaml", "talos-prod.yaml"];
    for name in config_paths {
        let content = std::fs::read_to_string(repo_root.join("conf").join(name))
            .expect("product config should be readable");
        let config: Config =
            serde_yaml::from_str(&content).expect("product config should deserialize");
        let raw: serde_json::Value =
            serde_json::to_value(serde_yaml::from_str::<serde_yaml::Value>(&content).unwrap())
                .unwrap();
        let mut effective = serde_json::to_value(&config).unwrap();
        for (section, field) in [("logging", "file"), ("storage", "connection_string")] {
            effective[section].as_object_mut().unwrap().remove(field);
        }
        effective["agent"]["kernel"]
            .as_object_mut()
            .unwrap()
            .remove("timezone");
        let mut missing = Vec::new();
        missing_defaults(&effective, &raw, "", &mut missing);
        assert!(
            missing.is_empty(),
            "{name} omits persisted defaults: {missing:?}"
        );
        assert_eq!(
            config
                .llm
                .deepseek
                .models
                .iter()
                .map(|model| (model.id.as_str(), model.context_window))
                .collect::<Vec<_>>(),
            vec![
                ("deepseek-flash", 1_048_576),
                ("deepseek-v4-pro", 1_048_576),
            ]
        );
        assert_eq!(config.llm.model, "deepseek-flash", "{name}");
        assert_eq!(
            config.agent.inner_model,
            Some(ModelRef {
                provider: "deepseek".to_owned(),
                id: "deepseek-flash".to_owned()
            }),
            "{name}"
        );
        config
            .validate_runtime()
            .expect("bundled selections must resolve in their catalog");
        for (_, provider) in config.llm.providers() {
            assert_eq!(
                provider.http_timeouts,
                Some(ProviderHttpTimeouts::default()),
                "{name}"
            );
        }
        for provider in ["openai", "anthropic", "deepseek", "openrouter"] {
            for model in &config.provider_config(provider).unwrap().models {
                if model
                    .reasoning_efforts
                    .as_ref()
                    .is_some_and(|efforts| efforts.contains(&ReasoningEffort::Max))
                {
                    model
                        .validate_reasoning_effort(Some(ReasoningEffort::Max))
                        .unwrap();
                    assert!(model
                        .selection_capabilities()
                        .reasoning_efforts
                        .contains(&ReasoningEffort::Max));
                }
            }
        }
        let qwen4 = config
            .llm
            .bailian
            .models
            .iter()
            .find(|model| model.id == "qwen4-max")
            .unwrap();
        assert_eq!(
            qwen4.selection_capabilities().reasoning_efforts,
            vec![
                ReasoningEffort::Light,
                ReasoningEffort::High,
                ReasoningEffort::XHigh
            ]
        );
        assert!(qwen4
            .validate_reasoning_effort(Some(ReasoningEffort::Max))
            .is_err());
        let openrouter_models = config
            .llm
            .openrouter
            .models
            .iter()
            .map(|model| (model.id.as_str(), model.context_window))
            .collect::<Vec<_>>();
        assert!(openrouter_models.contains(&("tencent/hy3:free", 262_144)));
        assert!(openrouter_models.contains(&("meta-llama/llama-3.3-70b-instruct", 131_072)));
        assert!(openrouter_models.contains(&("meta-llama/llama-3.3-70b-instruct:free", 131_072)));
        assert_eq!(
            config
                .llm
                .ollama
                .models
                .iter()
                .map(|model| {
                    (
                        model.id.as_str(),
                        model.context_window,
                        model.reasoning,
                        model.reasoning_effort,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("qwen3.8:27b", 262_144, true, ReasoningEffort::Medium),
                ("qwen3:8b", 40_960, true, ReasoningEffort::Medium),
                ("deepseek-r1:8b", 131_072, true, ReasoningEffort::Medium),
                ("gpt-oss:20b", 131_072, true, ReasoningEffort::Medium),
            ]
        );
    }
}

fn missing_defaults(
    effective: &serde_json::Value,
    raw: &serde_json::Value,
    path: &str,
    missing: &mut Vec<String>,
) {
    match effective {
        serde_json::Value::Object(fields) => {
            for (name, value) in fields {
                let next = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}.{name}")
                };
                if let Some(saved) = raw.get(name) {
                    missing_defaults(value, saved, &next, missing);
                } else {
                    missing.push(next);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let next = format!("{path}[{index}]");
                if let Some(saved) = raw.get(index) {
                    missing_defaults(item, saved, &next, missing);
                } else {
                    missing.push(next);
                }
            }
        }
        _ => {}
    }
}
