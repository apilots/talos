use super::*;

#[test]
fn embedded_session_context_uses_terminal_channel() {
    let app = TuiApp::build(None);
    let context = app.build_talos_context();

    assert_eq!(context.source_type, TalosSourceType::Cli);
    assert_eq!(context.provider, TalosProvider::Cli);
    assert_eq!(context.channel_id.as_deref(), Some("terminal"));
}
