use super::*;
use bytes::Bytes;
use std::collections::BTreeMap;
use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, TalosTurnStartedEvent, TimelineEventEnvelope,
};

#[test]
fn skips_cached_actor_credentials_for_local_session_methods() {
    let credentials = Some(ActorCredentials {
        credential: ActorCredential::ExistingSessionToken {
            token: "cloud-token".to_string(),
        },
    });

    assert_eq!(
        actor_credentials_for_method("initialize", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth.session.read", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth.session.clear", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth.login.start", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth.login.status", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth.login.cancel", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("session.list", &credentials),
        credentials
    );
}

#[test]
fn touches_cached_session_only_for_business_methods() {
    assert!(!is_business_method("initialize"));
    assert!(!is_business_method("auth.session.read"));
    assert!(!is_business_method("auth.session.clear"));
    assert!(!is_business_method("auth.login.start"));
    assert!(!is_business_method("auth.login.status"));
    assert!(!is_business_method("auth.login.cancel"));
    assert!(is_business_method("user.current"));
    assert!(is_business_method("profile/read"));
    assert!(is_business_method("session.list"));
}

#[test]
fn file_upload_params_use_canonical_padded_base64() {
    let params = files::file_upload_params(
        talos_types::FileUploadRequest {
            filename: "note.txt".to_string(),
            scope: talos_types::FileScope::Personal,
            tags: Vec::new(),
            metadata: BTreeMap::new(),
        },
        Bytes::from_static(b"ab"),
    );

    assert_eq!(params.data_base64, "YWI=");
    assert_eq!(params.request.filename, "note.txt");
}

#[tokio::test]
async fn cloud_login_status_uses_returned_token_without_session_cache() {
    let mut client = test_app_server_client(false);
    let response = authenticated_login_response("web-token");

    client
        .apply_cloud_login_credentials(&response)
        .await
        .expect("returned token should be accepted");

    assert_eq!(
        client.actor_credentials,
        Some(existing_session_credentials("web-token".to_string()))
    );
}

#[tokio::test]
async fn cloud_login_status_uses_returned_token_with_session_cache_enabled() {
    let mut client = test_app_server_client(true);
    let response = authenticated_login_response("local-token");

    client
        .apply_cloud_login_credentials(&response)
        .await
        .expect("returned token should avoid local cache read during login");

    assert_eq!(
        client.actor_credentials,
        Some(existing_session_credentials("local-token".to_string()))
    );
}

#[tokio::test]
async fn dispatch_application_event_routes_only_matching_subscription() {
    let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
    let pending_subscription_events = Arc::new(Mutex::new(HashMap::new()));
    let (first_tx, mut first_rx) = mpsc::channel(4);
    let (second_tx, mut second_rx) = mpsc::channel(4);
    {
        let mut streams = subscription_senders.lock().await;
        streams.insert(
            "sub-1".to_string(),
            SubscriptionStream {
                session_id: "session-1".to_string(),
                sender: first_tx,
            },
        );
        streams.insert(
            "sub-2".to_string(),
            SubscriptionStream {
                session_id: "session-1".to_string(),
                sender: second_tx,
            },
        );
    }

    dispatch_application_event(
        &subscription_senders,
        &pending_subscription_events,
        "sub-1",
        timeline_envelope("ev_1", 1),
    )
    .await;

    assert_eq!(first_rx.recv().await.expect("first event").event_id, "ev_1");
    assert!(second_rx.try_recv().is_err());
}

#[tokio::test]
async fn dispatch_application_event_buffers_until_stream_registration() {
    let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
    let pending_subscription_events = Arc::new(Mutex::new(HashMap::new()));

    dispatch_application_event(
        &subscription_senders,
        &pending_subscription_events,
        "sub-1",
        timeline_envelope("ev_1", 1),
    )
    .await;

    let buffered = pending_subscription_events
        .lock()
        .await
        .remove("sub-1")
        .expect("buffered event");
    assert_eq!(buffered.len(), 1);
    assert_eq!(buffered[0].event_id, "ev_1");
}

fn timeline_envelope(event_id: &str, event_seq: i64) -> TimelineEventEnvelope {
    let event = AgentEvent {
        event_seq: Some(event_seq),
        event_id: Some(event_id.to_string()),
        created_at_ms: Some(event_seq),
        event_type: AgentEventType::TurnStarted,
        turn_id: "turn-1".to_string(),
        message_id: "msg-1".to_string(),
        session_id: "session-1".to_string(),
        sequence: event_seq as u64,
        payload: AgentEventPayload::TurnStarted(TalosTurnStartedEvent {
            request_id: "req-1".to_string(),
            turn_id: "turn-1".to_string(),
            started_at_ms: event_seq as u64,
        }),
    };
    TimelineEventEnvelope {
        event_id: event_id.to_string(),
        event_seq,
        created_at_ms: event_seq,
        session_id: event.session_id.clone(),
        turn_id: event.turn_id.clone(),
        message_id: event.message_id.clone(),
        event,
    }
}

fn test_app_server_client(use_cloud_session_cache: bool) -> AppServerClient {
    let (_events_tx, events) = mpsc::channel(1);
    let (_notifications_tx, notifications) = mpsc::channel(1);
    AppServerClient {
        ownership: AppServerOwnership::BorrowedProcess,
        child: None,
        stdin: None,
        next_id: 1,
        pending_responses: Arc::new(Mutex::new(HashMap::new())),
        subscription_senders: Arc::new(Mutex::new(HashMap::new())),
        pending_subscription_events: Arc::new(Mutex::new(HashMap::new())),
        events,
        notifications,
        actor_credentials: None,
        use_cloud_session_cache,
    }
}

fn authenticated_login_response(access_token: &str) -> CloudLoginStatusResponse {
    CloudLoginStatusResponse::Authenticated {
        user: talos_auth::UserDto {
            user_id: "user-1".to_string(),
            username: "ada".to_string(),
            email: None,
            role: talos_auth::UserRole::User,
            r#type: talos_auth::UserType::Pro,
            disabled: false,
            created_at: 0,
            updated_at: 0,
        },
        token_type: "Bearer".to_string(),
        access_token: access_token.to_string(),
    }
}
