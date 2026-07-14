use super::*;
use crate::test_support::mock_model_info;
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn uses_disabled_none_for_model_without_reasoning() {
    let model = mock_model_info("plain-model", 128_000);
    let current = ModelRef {
        provider: "mock".to_string(),
        id: "plain-model".to_string(),
    };
    let mut picker = ModelPickerState::new(&[model], &current, None).unwrap();

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
    let picker = ModelPickerState::new(&[model], &current, None).unwrap();
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
            "Select Model and Effort",
            "",
            "› 1. mock / plain-model (current)    Fast model for everyday work.",
        ]
    );
}
