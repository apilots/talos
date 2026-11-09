use super::*;
use std::time::Duration;
use talos_app_server::{DesktopHostBridge, DesktopHostEvent};

#[tokio::test]
async fn remote_eof_sends_terminal_frame_to_desktop_host() {
    let host = DesktopHostBridge::new();
    host.activate();
    let mut events = host.take_receiver().unwrap();
    let media = DesktopMedia::new(host.clone());
    let (remote, incoming) = mpsc::channel(1);
    let (outgoing, _input) = mpsc::channel(1);
    let stream_id = uuid::Uuid::new_v4().to_string();
    media
        .start_stream(
            media
                .reserve(&Actor::regular("alice"), stream_id.clone())
                .unwrap(),
            talos_browser::DesktopTransport {
                incoming,
                outgoing,
                stopped: CancellationToken::new(),
            },
        )
        .unwrap();

    remote.send(Ok(b"RFB".to_vec())).await.unwrap();
    let Some(DesktopHostEvent::Media { id, frame }) =
        tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("Desktop host must receive remote data")
    else {
        panic!("expected a Desktop media frame");
    };
    assert_eq!(frame.stream_id, stream_id);
    assert_eq!(frame.data_base64.as_deref(), Some("UkZC"));
    host.complete(&id, Ok(serde_json::json!({"kind":"done"})));

    drop(remote);
    let Some(DesktopHostEvent::Media { id, frame }) =
        tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .expect("Desktop host must receive the terminal frame after remote EOF")
    else {
        panic!("expected a terminal Desktop media frame");
    };
    assert_eq!(frame.stream_id, stream_id);
    assert_eq!(frame.data_base64, None);
    host.complete(&id, Ok(serde_json::json!({"kind":"done"})));
    tokio::time::timeout(Duration::from_secs(1), async {
        while media.state.lock().unwrap().streams.contains_key(&stream_id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("terminal acknowledgement must release the viewer");
}

#[test]
fn pending_viewers_count_toward_capacity_and_close_is_owner_scoped() {
    let media = DesktopMedia::new(DesktopHostBridge::new());
    let owner = Actor::regular("alice");
    let other = Actor::regular("bob");
    let first_id = uuid::Uuid::new_v4().to_string();
    let first = media.reserve(&owner, first_id.clone()).unwrap();
    assert_eq!(
        media
            .reserve(&owner, first_id.clone())
            .err()
            .unwrap()
            .error_code(),
        ErrorCode::Conflict
    );
    let second = media
        .reserve(&owner, uuid::Uuid::new_v4().to_string())
        .unwrap();
    assert_eq!(
        media
            .reserve(&owner, uuid::Uuid::new_v4().to_string())
            .err()
            .unwrap()
            .error_code(),
        ErrorCode::Conflict
    );
    let close = || DesktopStreamClose {
        stream_id: first_id.clone(),
    };
    assert_eq!(
        media.close(&other, close()).unwrap_err().error_code(),
        ErrorCode::PermissionDenied
    );
    assert!(!first.stop.is_cancelled());
    media.close(&owner, close()).unwrap();
    assert!(first.stop.is_cancelled());

    let replacement = media.reserve(&owner, first_id.clone()).unwrap();
    let (_remote, incoming) = mpsc::channel(1);
    let (outgoing, _input) = mpsc::channel(1);
    let error = media
        .start_stream(
            first,
            talos_browser::DesktopTransport {
                incoming,
                outgoing,
                stopped: CancellationToken::new(),
            },
        )
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::PermissionDenied);
    assert!(!replacement.stop.is_cancelled());

    let suspension = media.suspend();
    assert!(second.stop.is_cancelled());
    assert!(replacement.stop.is_cancelled());
    assert_eq!(
        media
            .reserve(&owner, first_id.clone())
            .err()
            .unwrap()
            .error_code(),
        ErrorCode::PermissionDenied
    );
    drop(suspension);
    let new_viewer = media.reserve(&owner, first_id).unwrap();
    media.close_all();
    assert!(new_viewer.stop.is_cancelled());
}
