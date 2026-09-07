use super::credentials::actor_credentials_for_method;
use super::*;
use bytes::Bytes;
use std::collections::BTreeMap;
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentTurnRef, AgentTurnStarted, TimelineEvent,
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
        actor_credentials_for_method("auth/session/read", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth/session/clear", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth/login/start", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth/login/status", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth/login/cancel", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("session/list", &credentials),
        credentials
    );
}

#[test]
fn touches_cached_session_only_for_business_methods() {
    assert!(!is_business_method("initialize"));
    assert!(!is_business_method("auth/session/read"));
    assert!(!is_business_method("auth/session/clear"));
    assert!(!is_business_method("auth/login/start"));
    assert!(!is_business_method("auth/login/status"));
    assert!(!is_business_method("auth/login/cancel"));
    assert!(is_business_method("user/current"));
    assert!(is_business_method("profile/read"));
    assert!(is_business_method("session/list"));
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

#[test]
fn decodes_message_queue_change_notification_without_timeline_event() {
    let notification = JsonRpcNotification::new(
        "messageQueue/changed",
        Some(serde_json::json!({ "sessionId": "session-1", "revision": 6 })),
    );

    assert_eq!(
        response::decode_message_queue_changed(&notification).expect("notification should decode"),
        Some(talos_types::MessageQueueChangedNotification {
            session_id: "session-1".to_string(),
            revision: 6,
        })
    );
}

#[test]
fn decodes_slash_timeline_notification() {
    let expected = talos_app_protocol::events::TimelineEventNotification {
        subscription_id: "sub-1".to_string(),
        event: timeline_event("ev_1", 1),
    };
    let notification = JsonRpcNotification::new(
        "timeline/turn/started",
        Some(serde_json::to_value(&expected).expect("notification should serialize")),
    );

    let decoded = response::decode_event_notification(notification)
        .expect("notification should decode")
        .expect("timeline method should be recognized");

    assert_eq!(decoded.subscription_id, expected.subscription_id);
    assert_eq!(decoded.event.event_id, expected.event.event_id);
    assert_eq!(decoded.event.event, expected.event.event);
}

#[tokio::test]
async fn rejected_cached_credentials_preserve_the_original_request_error() {
    let mut child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("start local pipe fixture");
    let mut client = test_app_server_client(/*use_cloud_session_cache*/ true);
    client.stdin = child.stdin.take();
    client.actor_credentials = Some(existing_session_credentials("expired-token".to_string()));
    let expected = TalosError::from_source(
        talos_error::ErrorCode::Unauthenticated,
        "The login session has expired.",
        std::io::Error::other("private authentication diagnostic"),
    );
    let pending = client.pending_responses.clone();
    let mut lines = BufReader::new(child.stdout.take().expect("fixture stdout")).lines();
    let response_error = expected.clone();
    let respond = async move {
        let line = lines
            .next_line()
            .await
            .expect("read request")
            .expect("request line");
        let request: talos_app_protocol::jsonrpc::JsonRpcRequest =
            serde_json::from_str(&line).expect("request frame");
        let talos_app_protocol::jsonrpc::RequestId::Number(id) = request.id else {
            panic!("numeric SDK request ID");
        };
        pending
            .lock()
            .await
            .remove(&id)
            .expect("pending request")
            .send(Err(response_error))
            .expect("request receiver");
    };
    let (result, ()) = tokio::join!(client.request_value("session/list", ()), respond);
    let error = result.expect_err("expired login must fail");
    assert_eq!(error.to_api_error(), expected.to_api_error());
    assert!(std::ptr::eq(error.backtrace(), expected.backtrace()));
    assert!(client.actor_credentials.is_none());
    drop(client.stdin.take());
    child.wait().await.expect("fixture exit");
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
async fn dispatch_timeline_event_routes_only_matching_subscription() {
    let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
    let pending_subscription_events = Arc::new(Mutex::new(HashMap::new()));
    let (first_tx, mut first_rx) = mpsc::channel(4);
    let (second_tx, mut second_rx) = mpsc::channel(4);
    let (first_queue_tx, _first_queue_rx) = mpsc::channel(4);
    let (second_queue_tx, _second_queue_rx) = mpsc::channel(4);
    {
        let mut streams = subscription_senders.lock().await;
        streams.insert(
            "sub-1".to_string(),
            SubscriptionStream {
                session_id: "session-1".to_string(),
                sender: first_tx,
                message_queue_sender: first_queue_tx,
            },
        );
        streams.insert(
            "sub-2".to_string(),
            SubscriptionStream {
                session_id: "session-1".to_string(),
                sender: second_tx,
                message_queue_sender: second_queue_tx,
            },
        );
    }

    dispatch_timeline_event(
        &subscription_senders,
        &pending_subscription_events,
        "sub-1",
        timeline_event("ev_1", 1),
    )
    .await;

    assert_eq!(first_rx.recv().await.expect("first event").event_id, "ev_1");
    assert!(second_rx.try_recv().is_err());
}

#[tokio::test]
async fn dispatch_timeline_event_buffers_until_stream_registration() {
    let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
    let pending_subscription_events = Arc::new(Mutex::new(HashMap::new()));

    dispatch_timeline_event(
        &subscription_senders,
        &pending_subscription_events,
        "sub-1",
        timeline_event("ev_1", 1),
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

#[tokio::test]
async fn message_queue_change_closes_a_lagged_sdk_subscription() {
    let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
    let (timeline_tx, mut timeline_rx) = mpsc::channel(1);
    let (queue_tx, _queue_rx) = mpsc::channel(1);
    queue_tx
        .try_send(talos_types::MessageQueueChangedNotification {
            session_id: "session-1".to_string(),
            revision: 1,
        })
        .expect("fill queue channel");
    subscription_senders.lock().await.insert(
        "sub-1".to_string(),
        SubscriptionStream {
            session_id: "session-1".to_string(),
            sender: timeline_tx,
            message_queue_sender: queue_tx,
        },
    );

    dispatch_message_queue_change(
        &subscription_senders,
        talos_types::MessageQueueChangedNotification {
            session_id: "session-1".to_string(),
            revision: 2,
        },
    )
    .await;

    assert!(subscription_senders.lock().await.is_empty());
    assert!(timeline_rx.recv().await.is_none());
}

fn timeline_event(event_id: &str, event_seq: i64) -> TimelineEvent {
    TimelineEvent {
        event_id: event_id.to_string(),
        event_seq,
        persisted_at: event_seq,
        event: talos_agent_api::TimelineEventPayload::Agent(AgentEvent {
            identity: AgentTurnRef::new("session-1", "turn-1"),
            sequence: event_seq as u64,
            occurred_at: event_seq,
            payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        }),
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
