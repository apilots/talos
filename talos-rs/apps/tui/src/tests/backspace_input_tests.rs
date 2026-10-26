use super::*;

#[tokio::test]
async fn backspace_ctrl_h_removes_unicode_before_cursor() {
    let mut app = create_embedded_app();
    app.input_buffer = "a界b".to_owned();
    app.cursor_index = 2;
    app.handle_input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    assert_eq!(app.input_buffer, "ab");
    assert_eq!(app.cursor_index, 1);
}

#[tokio::test]
async fn backspace_ctrl_h_is_safe_at_start_and_empty_input() {
    let mut app = create_embedded_app();
    for text in ["abc", ""] {
        app.input_buffer = text.to_owned();
        app.cursor_index = 0;
        app.handle_input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL))
            .await
            .unwrap();
        assert_eq!(app.input_buffer, text);
        assert_eq!(app.cursor_index, 0);
    }
}

#[tokio::test]
async fn backspace_ctrl_h_updates_command_completion() {
    let mut app = create_embedded_app();
    app.input_buffer = "/mod".to_owned();
    app.cursor_index = 4;
    app.show_command_autocomplete = true;
    app.command_input_buffer = "mod".to_owned();
    app.handle_input(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL))
        .await
        .unwrap();
    assert_eq!(app.input_buffer, "/mo");
    assert_eq!(app.command_input_buffer, "mo");
    assert_eq!(app.cursor_index, 3);
}
