use super::*;

#[tokio::test]
async fn retained_queries_and_deletions_remain_authenticated_and_responsive_during_remote_wait() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let app = Arc::new(MockApp {
        blocked_model: Some((entered.clone(), release.clone())),
        ..Default::default()
    });
    let (client, server) = duplex(128 * 1024);
    let (client_read, mut client_write) = split(client);
    let (server_read, server_write) = split(server);
    let runtime = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig {
            auth_enabled: true,
            ..Default::default()
        },
    ));
    let mut output = BufReader::new(client_read).lines();
    client_write
        .write_all(initialize_request("unused").as_bytes())
        .await
        .unwrap();
    read_output_line(&mut output).await;
    let credentials = json!({"credential":{"type":"existing_session_token","token":"user-token"}});
    let remote = json!({"jsonrpc":"2.0","id":10,"method":"model/list","params":{"nodeId":"remote"},"actor_credentials":credentials});
    client_write
        .write_all(format!("{remote}\n").as_bytes())
        .await
        .unwrap();
    entered.notified().await;
    for (index, (method, params)) in [
        ("user/current", json!({})),
        ("sync/status", json!(null)),
        ("session/list", json!({})),
        ("session/get", json!({"sessionId":"s1"})),
        (
            "session/history",
            json!({"sessionId":"s1","cursor":null,"limit":null}),
        ),
        ("session/active_turn", json!({"sessionId":"s1"})),
        ("project/list", json!({})),
        ("project/read", json!({"project_id":"p1"})),
        ("session/delete", json!({"sessionId":"s1"})),
        ("project/delete", json!({"project_id":"p1"})),
    ]
    .into_iter()
    .enumerate()
    {
        let id = index + 20;
        let request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params,"actor_credentials":credentials});
        client_write
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            read_output_line(&mut output),
        )
        .await
        .unwrap_or_else(|_| panic!("{method} must finish before remote release"));
        assert_eq!(response["id"], id);
        assert_eq!(result_data(&response), &json!({"ok":true}));
    }
    // These trusted-host bootstrap reads preserve the initialized actor and
    // their original credential exemption; account data still requires a token.
    for (id, method, params) in [
        (40, "desktop/workerIdentity", json!(null)),
        (41, "auth/session/read", json!({})),
    ] {
        let request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        client_write
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            read_output_line(&mut output),
        )
        .await
        .expect("local Desktop bootstrap must not wait for the Worker");
        assert_eq!(response["id"], id);
        assert_eq!(result_data(&response), &json!({"ok":true}));
    }
    client_write
        .write_all(session_read_request_with_actor_token("bad").as_bytes())
        .await
        .unwrap();
    let denied = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        read_output_line(&mut output),
    )
    .await
    .unwrap();
    assert_eq!(result_error(&denied)["code"], "unauthenticated");
    let expected_actors = ["user-1"; 10]
        .into_iter()
        .chain(["anonymous"; 2])
        .collect::<Vec<_>>();
    assert_eq!(
        app.dispatched_actors.lock().unwrap().as_slice(),
        expected_actors
    );
    release.notify_one();
    assert_eq!(read_output_line(&mut output).await["id"], 10);
    client_write.shutdown().await.unwrap();
    runtime.await.unwrap().unwrap();
}

#[tokio::test]
async fn pending_deletion_capacity_cannot_starve_local_queries() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let app = Arc::new(MockApp {
        blocked_deletion: Some((entered.clone(), release.clone())),
        ..Default::default()
    });
    let (client, server) = duplex(128 * 1024);
    let (client_read, mut client_write) = split(client);
    let (server_read, server_write) = split(server);
    let runtime = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app,
        Arc::new(ExistingTokenValidator),
        AppServerConfig::default(),
    ));
    let mut output = BufReader::new(client_read).lines();
    client_write
        .write_all(initialize_request("unused").as_bytes())
        .await
        .unwrap();
    read_output_line(&mut output).await;
    for id in 10..14 {
        let request =
            json!({"jsonrpc":"2.0","id":id,"method":"session/delete","params":{"sessionId":"s1"}});
        client_write
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        entered.notified().await;
    }
    let query = json!({"jsonrpc":"2.0","id":20,"method":"session/list","params":{}});
    client_write
        .write_all(format!("{query}\n").as_bytes())
        .await
        .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        read_output_line(&mut output),
    )
    .await
    .expect("queries have independent capacity");
    assert_eq!(response["id"], 20);
    assert_eq!(result_data(&response), &json!({"ok":true}));
    let excess =
        json!({"jsonrpc":"2.0","id":21,"method":"session/delete","params":{"sessionId":"s1"}});
    client_write
        .write_all(format!("{excess}\n").as_bytes())
        .await
        .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        read_output_line(&mut output),
    )
    .await
    .unwrap();
    assert_eq!(response["id"], 21);
    assert_eq!(result_error(&response)["code"], "conflict");
    release.notify_waiters();
    let mut completed = Vec::new();
    for _ in 0..4 {
        completed.push(read_output_line(&mut output).await["id"].as_i64().unwrap());
    }
    completed.sort_unstable();
    assert_eq!(completed, vec![10, 11, 12, 13]);
    client_write.shutdown().await.unwrap();
    runtime.await.unwrap().unwrap();
}

#[tokio::test]
async fn local_live_subscription_and_unsubscription_progress_during_remote_wait() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let app = Arc::new(MockApp {
        blocked_model: Some((entered.clone(), release.clone())),
        ..Default::default()
    });
    let (live_tx, _live_rx) = broadcast::channel(16);
    *app.live_tx.lock().unwrap() = Some(live_tx);
    let (client, server) = duplex(128 * 1024);
    let (client_read, mut client_write) = split(client);
    let (server_read, server_write) = split(server);
    let runtime = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig {
            auth_enabled: true,
            ..Default::default()
        },
    ));
    let mut output = BufReader::new(client_read).lines();
    client_write
        .write_all(initialize_request("unused").as_bytes())
        .await
        .unwrap();
    read_output_line(&mut output).await;
    client_write
        .write_all(initialized_notification().as_bytes())
        .await
        .unwrap();
    let credentials = json!({"credential":{"type":"existing_session_token","token":"user-token"}});
    let remote = json!({"jsonrpc":"2.0","id":10,"method":"model/list","params":{"nodeId":"remote"},"actor_credentials":credentials});
    client_write
        .write_all(format!("{remote}\n").as_bytes())
        .await
        .unwrap();
    entered.notified().await;
    let mut request: serde_json::Value = serde_json::from_str(&event_subscribe_request()).unwrap();
    request["actor_credentials"] = credentials.clone();
    client_write
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        read_output_line(&mut output),
    )
    .await
    .expect("local Live subscription must not wait for remote RPC");
    assert_eq!(response["id"], 2);
    let subscription = result_data(&response)["subscriptionId"].as_str().unwrap();
    let sender = app.stream_txs.lock().unwrap()[0].clone();
    sender
        .send(TimelineSubscriptionMessage::Event(Box::new(
            timeline_event("ev_1", 1),
        )))
        .await
        .unwrap();
    let event = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        read_output_line(&mut output),
    )
    .await
    .expect("local Live events must continue during the remote wait");
    assert_eq!(event["params"]["event"]["eventId"], "ev_1");
    let mut unsubscribe: serde_json::Value =
        serde_json::from_str(&event_unsubscribe_request(3, subscription)).unwrap();
    unsubscribe["actor_credentials"] = credentials;
    client_write
        .write_all(format!("{unsubscribe}\n").as_bytes())
        .await
        .unwrap();
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        read_output_line(&mut output),
    )
    .await
    .expect("local Live cancellation must not wait for remote RPC");
    assert_eq!(response["id"], 3);
    assert_eq!(result_data(&response)["unsubscribed"], true);
    assert!(sender
        .send(TimelineSubscriptionMessage::Event(Box::new(
            timeline_event("ev_2", 2)
        )))
        .await
        .is_err());
    release.notify_one();
    assert_eq!(read_output_line(&mut output).await["id"], 10);
    client_write.shutdown().await.unwrap();
    runtime.await.unwrap().unwrap();
}

#[tokio::test]
async fn stdin_eof_cancels_stalled_local_requests_before_draining_the_application() {
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let app = Arc::new(MockApp {
        blocked_deletion: Some((entered.clone(), release.clone())),
        ..Default::default()
    });
    let (client, server) = duplex(128 * 1024);
    let (client_read, mut client_write) = split(client);
    let (server_read, server_write) = split(server);
    let mut runtime = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig::default(),
    ));
    let mut output = BufReader::new(client_read).lines();
    client_write
        .write_all(initialize_request("unused").as_bytes())
        .await
        .unwrap();
    read_output_line(&mut output).await;
    client_write.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":10,\"method\":\"session/delete\",\"params\":{\"sessionId\":\"stalled\"}}\n").await.unwrap();
    entered.notified().await;
    client_write.shutdown().await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), &mut runtime).await;
    if result.is_err() {
        // Release the fixture even on the regression path, without hiding the failure.
        release.notify_waiters();
        runtime.await.unwrap().unwrap();
        panic!("EOF must cancel local requests without waiting for remote release");
    }
    result.unwrap().unwrap().unwrap();
    assert!(
        app.dispatched.lock().unwrap().is_empty(),
        "the stalled request must be cancelled"
    );
    assert_eq!(*app.drained.lock().unwrap(), 1);
}
