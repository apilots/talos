use super::*;
use std::time::Duration;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

struct DesktopConnection {
    input: WriteHalf<tokio::io::DuplexStream>,
    output: tokio::io::Lines<BufReader<ReadHalf<tokio::io::DuplexStream>>>,
    runtime: JoinHandle<ProtocolResult<()>>,
    host: Arc<crate::DesktopHostBridge>,
}

impl DesktopConnection {
    async fn new(app: Arc<MockApp>) -> Self {
        let (client, server) = duplex(16 * 1024);
        let (client_read, input) = split(client);
        let (server_read, server_write) = split(server);
        let host = crate::DesktopHostBridge::new();
        let runtime = tokio::spawn(run_jsonl(
            server_read,
            server_write,
            app,
            Arc::new(ExistingTokenValidator),
            AppServerConfig {
                desktop_host: Some(host.clone()),
                ..Default::default()
            },
        ));
        let mut connection = Self {
            input,
            output: BufReader::new(client_read).lines(),
            runtime,
            host,
        };
        connection
            .input
            .write_all(initialize_request_for_kind("desktop").as_bytes())
            .await
            .unwrap();
        assert_eq!(connection.response().await["id"], 1);
        connection
            .input
            .write_all(initialized_notification().as_bytes())
            .await
            .unwrap();
        connection
    }

    async fn request(&mut self, id: u64, method: &str, params: serde_json::Value) {
        let line = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        self.input
            .write_all(format!("{line}\n").as_bytes())
            .await
            .unwrap();
    }

    async fn open_viewer(&mut self, id: u64) {
        self.request(
            id,
            "desktop/streamOpen",
            json!({
                "streamId": "71f38d28-9990-4674-a4ce-b8c42e3c65c7",
                "ticket": "119dcd45-351f-4a7c-a27b-9be30b46ced3",
            }),
        )
        .await;
    }

    async fn interrupt(&mut self, id: u64) {
        self.request(
            id,
            "turn/interrupt",
            json!({"identity":{"sessionId":"s1","turnId":"turn-1"}}),
        )
        .await;
    }

    async fn response(&mut self) -> serde_json::Value {
        tokio::time::timeout(Duration::from_secs(2), read_output_line(&mut self.output))
            .await
            .expect("expected App Server output before releasing the held request")
    }

    async fn finish(mut self) {
        self.input.shutdown().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), self.runtime)
            .await
            .expect("App Server shutdown")
            .unwrap()
            .unwrap();
    }
}

async fn wait_for(notification: &Notify) {
    tokio::time::timeout(Duration::from_secs(2), notification.notified())
        .await
        .expect("request did not reach its controlled wait");
}

#[tokio::test]
async fn desktop_admission_and_conversation_commands_progress_independently() {
    let viewer_entered = Arc::new(Notify::new());
    let viewer_release = Arc::new(Notify::new());
    let turn_entered = Arc::new(Notify::new());
    let turn_release = Arc::new(Notify::new());
    let (live_tx, _) = broadcast::channel(16);
    let app = Arc::new(MockApp {
        blocked_desktop: Some((viewer_entered.clone(), viewer_release.clone())),
        blocked_turn: Some((turn_entered.clone(), turn_release.clone())),
        live_tx: Mutex::new(Some(live_tx)),
        ..Default::default()
    });
    let mut client = DesktopConnection::new(app.clone()).await;
    client
        .input
        .write_all(event_subscribe_request_with_id(9).as_bytes())
        .await
        .unwrap();
    assert_eq!(client.response().await["id"], 9);

    client.open_viewer(10).await;
    wait_for(&viewer_entered).await;
    client
        .input
        .write_all(turn_start_request().as_bytes())
        .await
        .unwrap();
    wait_for(&turn_entered).await;
    client.interrupt(11).await;
    assert_eq!(result_data(&client.response().await), &json!({"ok":true}));
    client
        .request(12, "timeline/recover", json!({"sessionId":"s1"}))
        .await;
    let recovered = client.response().await;
    assert_eq!(recovered["id"], 12);
    assert_eq!(result_data(&recovered), &json!({"ok":true}));
    let stream = app.stream_txs.lock().unwrap()[0].clone();
    stream
        .send(TimelineSubscriptionMessage::Event(Box::new(
            timeline_event("evt-live", 1),
        )))
        .await
        .unwrap();
    assert_eq!(
        client.response().await["params"]["event"]["eventId"],
        "evt-live"
    );

    turn_release.notify_one();
    let turn = client.response().await;
    assert_eq!(turn["id"], 2);
    assert_eq!(result_data(&turn)["turnId"], "turn-1");
    viewer_release.notify_one();
    assert_eq!(client.response().await["id"], 10);

    client
        .input
        .write_all(turn_start_request().as_bytes())
        .await
        .unwrap();
    wait_for(&turn_entered).await;
    client.open_viewer(13).await;
    wait_for(&viewer_entered).await;
    viewer_release.notify_one();
    assert_eq!(client.response().await["id"], 13);
    turn_release.notify_one();
    assert_eq!(client.response().await["id"], 2);
    client.finish().await;
}

#[tokio::test]
async fn saturated_general_requests_leave_control_and_host_replies_available() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let app = Arc::new(MockApp {
        blocked_remote: Some((entered.clone(), release.clone())),
        ..Default::default()
    });
    let mut client = DesktopConnection::new(app).await;
    for id in 10..26 {
        client.request(id, "model/list", json!({})).await;
        wait_for(&entered).await;
    }
    client.request(26, "model/list", json!({})).await;
    let rejected = client.response().await;
    assert_eq!(rejected["id"], 26);
    assert_eq!(result_error(&rejected)["code"], "conflict");

    client.interrupt(27).await;
    let interrupted = client.response().await;
    assert_eq!(interrupted["id"], 27);
    assert_eq!(result_data(&interrupted), &json!({"ok":true}));

    let host = client.host.clone();
    let media = tokio::spawn(async move {
        host.media(talos_types::DesktopMediaFrame {
            stream_id: "stream".into(),
            data_base64: None,
        })
        .await
    });
    let frame = client.response().await;
    assert_eq!(frame["method"], "client/desktop/frame");
    let ack = json!({"jsonrpc":"2.0","id":frame["id"],"result":{"kind":"done"}});
    client
        .input
        .write_all(format!("{ack}\n").as_bytes())
        .await
        .unwrap();
    media.await.unwrap().unwrap();

    release.notify_waiters();
    let mut replies = Vec::new();
    for _ in 10..26 {
        replies.push(client.response().await["id"].as_u64().unwrap());
    }
    replies.sort_unstable();
    assert_eq!(replies, (10..26).collect::<Vec<_>>());
    client.finish().await;
}

#[tokio::test]
async fn disconnect_cancels_a_turn_admitted_after_stdin_eof() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let closed = Arc::new(Notify::new());
    let app = Arc::new(MockApp {
        blocked_turn: Some((entered.clone(), release.clone())),
        connection_closed: Some(closed.clone()),
        ..Default::default()
    });
    let mut client = DesktopConnection::new(app.clone()).await;
    client
        .input
        .write_all(turn_start_request().as_bytes())
        .await
        .unwrap();
    wait_for(&entered).await;
    client.input.shutdown().await.unwrap();
    wait_for(&closed).await;
    release.notify_one();
    assert_eq!(client.response().await["id"], 2);
    client.finish().await;
    assert_eq!(*app.cancelled_turns.lock().unwrap(), ["turn-1"]);
}
