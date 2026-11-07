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
            "alice".into(),
            stream_id.clone(),
            talos_browser::DesktopTransport {
                incoming,
                outgoing,
                stopped: CancellationToken::new(),
            },
        )
        .await;

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
    assert!(!media.streams.lock().await.contains_key(&stream_id));
    host.complete(&id, Ok(serde_json::json!({"kind":"done"})));
}
