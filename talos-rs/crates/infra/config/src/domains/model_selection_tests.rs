use super::*;
use serde_json::json;

fn model(parameters: serde_json::Value) -> ProviderModelConfig {
    serde_json::from_value(json!({"id":"model","reasoning":true,"reasoning_effort":"high","request_parameters":parameters})).unwrap()
}

#[test]
fn explicit_wire_controls_determine_capabilities_and_defaults() {
    let configured =
        model(json!({"service_tier":"priority","chat_template_kwargs":{"enable_thinking":false}}));
    assert_eq!(
        configured.selection_capabilities(),
        ModelSelectionCapabilities {
            reasoning: ReasoningControl::Toggle,
            fast: true,
            default_fast: false,
            default_reasoning_effort: None,
        }
    );
    assert_eq!(
        model(json!({"service_tier":"default"})).selection_capabilities(),
        ModelSelectionCapabilities {
            reasoning: ReasoningControl::Effort,
            fast: true,
            default_fast: false,
            default_reasoning_effort: Some(ReasoningEffort::High),
        }
    );
    for tier in ["default", "fast", "priority"] {
        let mut configured = model(json!({"service_tier":tier}));
        assert!(!configured.default_fast_mode());
        configured.speed = ModelSpeed::Fast;
        assert!(configured.default_fast_mode());
    }
    for value in [json!(null), json!("auto"), json!("flex"), json!(true)] {
        assert!(!model(json!({"service_tier":value})).supports_fast_mode());
    }
    let mut static_speed = model(json!({}));
    static_speed.speed = super::super::network::ModelSpeed::Fast;
    assert!(!static_speed.supports_fast_mode());
    assert!(!static_speed.default_fast_mode());
}

#[test]
fn reasoning_controls_use_configured_binary_modes_without_inventing_adaptive_semantics() {
    for parameters in [
        json!({"enable_thinking":true}),
        json!({"thinking":{"type":"enabled"}}),
    ] {
        let capabilities = model(parameters).selection_capabilities();
        assert_eq!(capabilities.reasoning, ReasoningControl::Toggle);
        assert_eq!(
            capabilities.default_reasoning_effort,
            Some(ReasoningEffort::High)
        );
    }
    let capabilities = model(json!({"thinking":{"type":"disabled"}})).selection_capabilities();
    assert_eq!(capabilities.reasoning, ReasoningControl::Toggle);
    assert_eq!(capabilities.default_reasoning_effort, None);
    assert_eq!(
        model(json!({"thinking":{"type":"adaptive"}}))
            .selection_capabilities()
            .reasoning,
        ReasoningControl::Effort
    );
    let mut disabled = model(json!({"enable_thinking":true}));
    disabled.reasoning = false;
    assert_eq!(
        disabled.selection_capabilities().reasoning,
        ReasoningControl::None
    );
    assert_eq!(
        disabled.selection_capabilities().default_reasoning_effort,
        None
    );
}

#[test]
fn contradictory_binary_defaults_are_rejected() {
    for parameters in [
        json!({"enable_thinking":true,"chat_template_kwargs":{"enable_thinking":false}}),
        json!({"enable_thinking":false,"thinking":{"type":"enabled"}}),
    ] {
        assert!(model(parameters)
            .validate_selection_controls()
            .unwrap_err()
            .is_code(ErrorCode::InvalidRequest));
    }
    model(json!({"enable_thinking":false,"thinking":{"type":"adaptive"}}))
        .validate_selection_controls()
        .unwrap();
}
