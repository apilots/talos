use super::*;

#[tokio::test]
async fn desktop_browser_requests_cross_jsonl_and_correlate_only_the_original_response() {
    use talos_browser::BrowserBackend;
    let host = crate::DesktopHostBridge::new();
    let backend = talos_browser::NativeBrowser::new(host.clone());
    let (client, server) = duplex(128 * 1024);
    let (client_read, mut client_write) = split(client);
    let (server_read, server_write) = split(server);
    let runtime = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        Arc::new(MockApp::default()),
        Arc::new(RejectingValidator),
        AppServerConfig {
            desktop_host: Some(host),
            ..Default::default()
        },
    ));
    let mut output = BufReader::new(client_read).lines();
    client_write
        .write_all(desktop_host_initialize_request().as_bytes())
        .await
        .expect("initialize");
    let initialized = read_output_line(&mut output).await;
    assert!(initialized.get("result").is_some());
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !matches!(
            backend.readiness(talos_types::BrowserTargetKind::Managed),
            talos_types::BrowserReadiness::Ready
        ) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("trusted Desktop initialized");
    let request = tokio::spawn(async move { backend.open_local("alice".into()).await });
    let outgoing = read_output_line(&mut output).await;
    assert_eq!(outgoing["method"], "client/browser/execute");
    assert_eq!(outgoing["params"]["userId"], "alice");
    assert_eq!(outgoing["params"]["operation"]["kind"], "show");
    let wrong = json!({"jsonrpc":"2.0","id":"wrong-response","result":{"kind":"done"}});
    client_write
        .write_all(format!("{wrong}\n").as_bytes())
        .await
        .expect("wrong response");
    tokio::task::yield_now().await;
    assert!(!request.is_finished());
    let response = json!({"jsonrpc":"2.0","id":outgoing["id"],"result":{"kind":"done"}});
    client_write
        .write_all(format!("{response}\n").as_bytes())
        .await
        .expect("response");
    request
        .await
        .expect("request task")
        .expect("correlated response");
    client_write.shutdown().await.expect("shutdown input");
    runtime
        .await
        .expect("runtime task")
        .expect("runtime shutdown");
}

#[tokio::test]
async fn business_request_does_not_block_its_own_host_request_or_response() {
    let host = crate::DesktopHostBridge::new();
    let app = Arc::new(MockApp {
        desktop_host: Some(talos_browser::NativeBrowser::new(host.clone())),
        ..Default::default()
    });
    let (client, server) = duplex(128 * 1024);
    let (client_read, mut client_write) = split(client);
    let (server_read, server_write) = split(server);
    let runtime = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app,
        Arc::new(RejectingValidator),
        AppServerConfig {
            desktop_host: Some(host),
            ..Default::default()
        },
    ));
    let mut output = BufReader::new(client_read).lines();
    client_write
        .write_all(desktop_host_initialize_request().as_bytes())
        .await
        .expect("initialize");
    read_output_line(&mut output).await;
    let browser = json!({"jsonrpc":"2.0","id":2,"method":"browser/operate","params":{"nodeId":"embedded","operation":{"kind":"read"}}});
    client_write
        .write_all(format!("{browser}\n").as_bytes())
        .await
        .expect("browser read");
    let outgoing = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_output_line(&mut output),
    )
    .await
    .expect("host request while business dispatch waits");
    assert_eq!(outgoing["method"], "client/browser/execute");
    for id in 3..7 {
        let queued = json!({"jsonrpc":"2.0","id":id,"method":"session/read","params":{"sessionId":"queued"}});
        client_write
            .write_all(format!("{queued}\n").as_bytes())
            .await
            .expect("queue request");
    }
    let response = json!({"jsonrpc":"2.0","id":outgoing["id"],"result":{"kind":"done"}});
    client_write
        .write_all(format!("{response}\n").as_bytes())
        .await
        .expect("host response");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_output_line(&mut output),
    )
    .await
    .expect("business response");
    assert_eq!(result["id"], 2);
    assert!(result.get("result").is_some());
    for id in 3..7 {
        assert_eq!(read_output_line(&mut output).await["id"], id);
    }
    client_write.shutdown().await.expect("shutdown");
    runtime.await.expect("runtime").expect("result");
}

fn desktop_host_initialize_request() -> String {
    let mut request: serde_json::Value =
        serde_json::from_str(&initialize_request("secret")).expect("initialize");
    request["params"]["client_info"]["kind"] = json!("desktop");
    format!("{request}\n{}", initialized_notification())
}
