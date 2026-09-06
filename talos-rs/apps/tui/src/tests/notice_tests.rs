use super::*;
use talos_sdk::{
    AgentSystemNotice, SystemNoticeContent, TimelineEventPayload, TimelineSystemNotice,
};

#[test]
fn late_notice_preserves_success_and_history_deduplicates_session_delivery() {
    let mut app = create_embedded_app();
    app.session_id = Some("sess-test".to_string());
    let notice = TimelineSystemNotice {
        identity: talos_sdk::AgentTurnRef::new("sess-test", "turn-history"),
        occurred_at: 10,
        notice: AgentSystemNotice {
            level: talos_sdk::TalosTimelineNoticeLevel::Warning,
            content: SystemNoticeContent::Error(
                talos_error::TalosError::internal("Usage could not be saved.").to_api_error(),
            ),
        },
    };
    let envelope = talos_sdk::TimelineEvent {
        event_id: "ev_10".into(),
        event_seq: 10,
        persisted_at: 10,
        event: TimelineEventPayload::SystemNotice(notice.clone()),
    };
    let mut history = completed_history_response(
        vec![completed_text_message(
            "answer",
            talos_sdk::AgentMessageRole::Agent,
            "Saved answer",
        )],
        None,
    );
    history.data[0].notices.push(envelope.clone());
    let mut before_notice = history.clone();
    before_notice.data[0].notices.clear();
    app.replay_history_snapshot(before_notice);
    app.handle_stream_event(envelope);
    app.apply_system_notice(10, notice);
    assert_eq!(
        (app.in_flight, app.active_stream_turn_id.as_deref()),
        (false, None)
    );
    assert_eq!(
        interactive_transcript_text(&app),
        "Saved answer\n\nWarning: Usage could not be saved."
    );
    app.render_runtime.reset_transcript();
    app.replay_history_snapshot(history);
    assert_eq!(
        interactive_transcript_text(&app),
        "Saved answer\n\nWarning: Usage could not be saved."
    );
}
