use super::*;
use serde_json::json;

#[test]
fn extension_values_preserve_json_types_and_replace_once() {
    let parameters = BTreeMap::from([
        ("thinking_budget".into(), json!(2048)),
        ("enable_thinking".into(), json!(false)),
        (
            "chat_template_kwargs".into(),
            json!({"enable_thinking":true,"mode":["a",1]}),
        ),
    ]);
    let body = serialize_with_parameters(
        json!({"model":"model","messages":[],"enable_thinking":true}),
        &parameters,
    )
    .unwrap();
    assert_eq!(
        body,
        json!({"model":"model","messages":[],"enable_thinking":false,"thinking_budget":2048,"chat_template_kwargs":{"enable_thinking":true,"mode":["a",1]}})
    );
}

#[test]
fn session_thinking_selection_overrides_examples_without_dropping_other_template_keys() {
    let mut parameters = BTreeMap::from([
        (
            "chat_template_kwargs".into(),
            json!({"enable_thinking":true,"other":7}),
        ),
        ("enable_thinking".into(), json!(true)),
        (
            "thinking".into(),
            json!({"type":"enabled","budget_tokens":100}),
        ),
    ]);
    apply_thinking_selection(&mut parameters, false);
    assert_eq!(
        parameters,
        BTreeMap::from([
            (
                "chat_template_kwargs".into(),
                json!({"enable_thinking":false,"other":7})
            ),
            ("enable_thinking".into(), json!(false)),
            (
                "thinking".into(),
                json!({"type":"disabled","budget_tokens":100})
            ),
        ])
    );
    assert!(has_boolean_thinking_control(&parameters));
    apply_thinking_selection(&mut parameters, true);
    assert_eq!(parameters["chat_template_kwargs"]["enable_thinking"], true);
}

#[test]
fn runtime_fields_cannot_be_replaced_by_wire_extensions() {
    for key in [
        "model",
        "messages",
        "input",
        "system",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "stream",
        "stream_options",
        "api_key",
        "authorization",
    ] {
        let error =
            serialize_with_parameters(json!({}), &BTreeMap::from([(key.to_owned(), json!(null))]))
                .unwrap_err();
        assert_eq!(error.error_code(), ErrorCode::InvalidRequest);
    }
}

#[test]
fn non_binary_thinking_modes_remain_explicit_wire_configuration() {
    let original = BTreeMap::from([(
        "thinking".into(),
        json!({"type":"adaptive", "display":"summarized"}),
    )]);
    for enabled in [false, true] {
        let mut parameters = original.clone();
        apply_thinking_selection(&mut parameters, enabled);
        assert_eq!(parameters, original);
    }
}
