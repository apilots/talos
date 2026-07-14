use super::*;
use crate::test_support::mock_model_info;

#[test]
fn uses_disabled_none_for_model_without_reasoning() {
    let model = mock_model_info("plain-model", 128_000);
    let current = ModelRef {
        provider: "mock".to_string(),
        id: "plain-model".to_string(),
    };
    let mut picker = ModelPickerState::new(&[model], &current, None);

    assert!(picker.confirm().is_none());
    let selection = picker.confirm().expect("none selection should confirm");

    assert_eq!(selection.model_ref, current);
    assert_eq!(selection.reasoning_effort, None);
}
