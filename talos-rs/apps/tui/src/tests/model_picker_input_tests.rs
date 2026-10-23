use super::*;
use talos_sdk::ReasoningEffort;

#[tokio::test]
async fn test_model_picker_enters_reasoning_step_and_esc_returns_to_model_step() {
    let mut app = create_embedded_app();
    set_test_model_info(&mut app, "gpt-5.5", 128_000);
    app.available_model_info[0].model.reasoning = true;
    app.available_model_info[0].model.reasoning_effort = ReasoningEffort::High;
    app.available_model_info[0].selection_capabilities =
        app.available_model_info[0].model.selection_capabilities();
    app.current_model = "gpt-5.5".to_string();
    app.reasoning_effort_override = Some(ReasoningEffort::High);

    app.toggle_model_picker();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();

    let mut picker = app.model_picker.clone().expect("model picker should open");
    assert!(!picker.back());

    app.handle_input(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .unwrap();

    let mut picker = app
        .model_picker
        .clone()
        .expect("model picker should stay open");
    assert!(picker.back());
}

#[tokio::test]
async fn test_model_picker_confirm_writes_command_result_block() {
    let mut app = create_embedded_app();
    set_test_model_info(&mut app, "gpt-5.5", 128_000);
    app.available_model_info[0].model.reasoning = true;
    app.available_model_info[0].model.reasoning_effort = ReasoningEffort::Medium;
    app.available_model_info[0].selection_capabilities =
        app.available_model_info[0].model.selection_capabilities();
    app.current_model = "gpt-5.5".to_string();
    app.reasoning_effort_override = Some(ReasoningEffort::Medium);

    app.toggle_model_picker();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();

    let transcript = interactive_transcript_text(&app);
    assert!(app.model_picker.is_none());
    assert_eq!(app.reasoning_effort_override, Some(ReasoningEffort::High));
    assert!(transcript.contains("Model changed to gpt-5.5 high"));

    app.toggle_model_picker();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    for _ in 0..2 {
        app.handle_input(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .await
            .unwrap();
    }
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();

    assert_eq!(app.reasoning_effort_override, Some(ReasoningEffort::Light));
    assert_eq!(
        app.current_model_selection().unwrap().reasoning_effort,
        Some(ReasoningEffort::Light)
    );
    app.toggle_model_picker();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
        .await
        .unwrap();
    app.handle_input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        .await
        .unwrap();
    assert_eq!(
        app.current_model_selection().unwrap().reasoning_effort,
        None
    );
    assert!(app.current_model_status_label().ends_with("none"));
}
