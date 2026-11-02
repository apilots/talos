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
            reasoning: ReasoningControl::Effort,
            reasoning_efforts: vec![
                ReasoningEffort::Light,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::XHigh
            ],
            fast: true,
            default_fast: false,
            default_reasoning_effort: None,
        }
    );
    assert_eq!(
        model(json!({"service_tier":"default"})).selection_capabilities(),
        ModelSelectionCapabilities {
            reasoning: ReasoningControl::Effort,
            reasoning_efforts: vec![
                ReasoningEffort::Light,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::XHigh
            ],
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
fn thinking_switches_do_not_restrict_effort_capabilities() {
    for parameters in [
        json!({"enable_thinking":true}),
        json!({"thinking":{"type":"enabled"}}),
    ] {
        let capabilities = model(parameters).selection_capabilities();
        assert_eq!(capabilities.reasoning, ReasoningControl::Effort);
        assert_eq!(
            capabilities.default_reasoning_effort,
            Some(ReasoningEffort::High)
        );
    }
    let capabilities = model(json!({"thinking":{"type":"disabled"}})).selection_capabilities();
    assert_eq!(capabilities.reasoning, ReasoningControl::Effort);
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

#[test]
fn explicit_efforts_project_defaults_and_supported_selections() {
    let mut configured = model(json!({"enable_thinking":true}));
    configured.reasoning_efforts = Some(vec![
        ReasoningEffort::Light,
        ReasoningEffort::Medium,
        ReasoningEffort::XHigh,
    ]);
    configured.reasoning_effort = ReasoningEffort::Medium;
    configured.validate_selection_controls().unwrap();
    assert_eq!(
        configured.selection_capabilities().reasoning,
        ReasoningControl::Effort
    );
    configured
        .validate_reasoning_effort(Some(ReasoningEffort::XHigh))
        .unwrap();
    configured.reasoning_efforts = Some(vec![ReasoningEffort::Medium, ReasoningEffort::Medium]);
    configured.validate_selection_controls().unwrap();
    assert_eq!(
        configured.selection_capabilities().reasoning_efforts,
        vec![ReasoningEffort::Medium]
    );
    configured.reasoning_efforts = Some(vec![]);
    configured.validate_selection_controls().unwrap();
    assert_eq!(
        configured.selection_capabilities().reasoning,
        ReasoningControl::Toggle
    );
    configured
        .validate_reasoning_effort(Some(ReasoningEffort::Medium))
        .unwrap();
    configured.request_parameters.clear();
    assert_eq!(
        configured.selection_capabilities().reasoning,
        ReasoningControl::None
    );
    configured.reasoning = false;
    configured.reasoning_efforts = Some(vec![ReasoningEffort::Light]);
    configured.validate_selection_controls().unwrap();
}
