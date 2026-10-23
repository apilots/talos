use super::*;
use crate::test_support::mock_model_info;
use ratatui::{backend::TestBackend, Terminal};
use talos_sdk::ModelRef;

#[test]
fn uses_disabled_none_for_model_without_reasoning() {
    let model = mock_model_info("plain-model", 128_000);
    let current = ModelRef {
        provider: "mock".to_string(),
        id: "plain-model".to_string(),
    };
    let mut picker = ModelPickerState::new(
        &[model],
        &ModelSelection::without_reasoning(current.clone()),
    )
    .unwrap();

    assert!(picker.confirm().is_none());
    let selection = picker.confirm().expect("none selection should confirm");

    assert_eq!(selection.model_ref, current);
    assert_eq!(selection.reasoning_effort, None);
}

#[test]
fn renders_model_catalog_with_compact_header() {
    let model = mock_model_info("plain-model", 128_000);
    let current = ModelRef {
        provider: "mock".to_string(),
        id: "plain-model".to_string(),
    };
    let picker = ModelPickerState::new(
        &[model],
        &ModelSelection::without_reasoning(current.clone()),
    )
    .unwrap();
    let backend = TestBackend::new(72, 8);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| picker.render(frame, frame.area(), None))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let lines = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        &lines[..3],
        [
            "Select Model Options",
            "",
            "› 1. mock / plain-model (current)    General-purpose model.",
        ]
    );
}

#[test]
fn reasoning_can_be_disabled_and_custom_labels_never_replace_routing_identity() {
    let mut model = mock_model_info("model", 128_000);
    model.provider = "llm-gateway".to_owned();
    model.provider_name = Some("LLM Gateway".to_owned());
    model.model.reasoning = true;
    model.selection_capabilities = model.model.selection_capabilities();
    let current = ModelRef {
        provider: "llm-gateway".to_owned(),
        id: "model".to_owned(),
    };
    let mut picker = ModelPickerState::new(
        &[model],
        &ModelSelection::without_reasoning(current.clone()),
    )
    .unwrap();
    assert!(picker.confirm().is_none());
    let selection = picker.confirm().unwrap();
    assert_eq!(selection.model_ref, current);
    assert_eq!(selection.reasoning_effort, None);
}

fn configured_model(parameters: serde_json::Value) -> ModelInfo {
    let mut model = mock_model_info("model", 128_000);
    model.model.reasoning = true;
    model.model.request_parameters = serde_json::from_value(parameters).unwrap();
    model.selection_capabilities = model.model.selection_capabilities();
    model
}

#[test]
fn configured_binary_reasoning_has_only_off_and_on_and_honors_off_default() {
    let model = configured_model(serde_json::json!({
        "chat_template_kwargs": { "enable_thinking": false }
    }));
    let selection = ModelSelection::from_model(
        ModelRef {
            provider: model.provider.clone(),
            id: model.model.id.clone(),
        },
        &model.model,
    );
    let mut picker = ModelPickerState::new(&[model], &selection).unwrap();
    assert_eq!(
        picker
            .reasoning_options()
            .iter()
            .map(|option| option.label)
            .collect::<Vec<_>>(),
        ["Off", "On"]
    );
    assert!(picker.confirm().is_none());
    assert_eq!(picker.confirm(), Some(selection.clone()));
    picker.move_selection(1);
    let enabled = picker.confirm().unwrap();
    assert_eq!(enabled.model_ref, selection.model_ref);
    assert_eq!(enabled.reasoning_effort, Some(ReasoningEffort::Medium));
}

#[test]
fn identical_model_ids_keep_provider_specific_reasoning_defaults() {
    let mut first = configured_model(serde_json::json!({"enable_thinking": true}));
    first.provider = "llm-gateway".to_string();
    let mut second = configured_model(serde_json::json!({"enable_thinking": false}));
    second.provider = "custom-test".to_string();
    let current = ModelSelection::from_model(
        ModelRef {
            provider: first.provider.clone(),
            id: first.model.id.clone(),
        },
        &first.model,
    );
    let mut picker = ModelPickerState::new(&[first, second], &current).unwrap();
    picker.move_selection(1);
    assert!(picker.confirm().is_none());
    let selected = picker.confirm().unwrap();
    assert_eq!(selected.model_ref.provider, "custom-test");
    assert_eq!(selected.model_ref.id, "model");
    assert_eq!(selected.reasoning_effort, None);
}
