use super::*;

struct ScopedFileCleanup {
    path: PathBuf,
}

impl ScopedFileCleanup {
    fn new(path: PathBuf) -> Self {
        let _ = std::fs::remove_file(&path);
        Self { path }
    }
}

impl Drop for ScopedFileCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[tokio::test]
#[ignore = "slow real-llm embedded flow"]
async fn test_embedded_quit_flushes_memory_after_runtime_shutdown() {
    dotenv().ok();
    if !real_llm_env_available() {
        return;
    }
    let cwd_memory = std::env::current_dir()
        .expect("current dir should resolve")
        .join("MEMORY.md");
    let _cleanup_guard = ScopedFileCleanup::new(cwd_memory);

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config_path = manifest_dir.join("../../conf/talos.yaml");
    let workspace_dir = unique_test_dir("embedded-quit-workspace");
    let data_dir = unique_test_dir("embedded-quit-data");
    let db_path = data_dir.join("talos.db");
    let port = find_free_port().await;

    let mut config = Config::load_from_path(Some(
        config_path
            .to_str()
            .expect("config path should be valid utf-8"),
    ))
    .expect("config should load");
    config.gateway.host = "127.0.0.1".to_string();
    config.gateway.port = port;
    config.gateway.auth = false;
    config.gateway.token = Some("embedded-test-token".to_string());
    config.logging.stdout = false;
    config.llm.model = "glm-5".to_string();
    config.storage.connection_string = db_path.to_string_lossy().to_string();
    config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config
        .validate_runtime()
        .expect("runtime config should be valid");

    let options = TalosServerOptions {
        host: Some("127.0.0.1".to_string()),
        port: Some(port),
        logging_stdout: Some(false),
        auth_disabled: Some(true),
        enable_channels: Some(false),
    };
    let server = start_server(config, Some(options))
        .await
        .expect("embedded runtime should start");
    let handle = server.handle();
    let gateway_url = server.gateway_url().to_string();
    let server_task = tokio::spawn(async move { server.run().await });

    tokio::time::sleep(Duration::from_millis(300)).await;

    let client = Arc::new(
        TalosClient::new(ClientConfig {
            gateway_url,
            auth_token: None,
            timeout: 30,
        })
        .expect("client should create"),
    );
    let mut app = TuiApp::new_with_client(client, TalosConnectionMode::Embedded);
    app.initialize_remote().await;

    app.input_buffer =
        "Remember this stable preference: I prefer oolong tea over coffee. Reply with exactly: embedded-quit-ok"
            .to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let should_quit = app
        .submit_input()
        .await
        .expect("message submit should succeed");
    assert!(!should_quit);

    let message_deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    while tokio::time::Instant::now() < message_deadline {
        app.on_tick().await;
        if !app.in_flight
            && app.message_queue.is_empty()
            && app.pending_stream.is_none()
            && app.pending_stream_connect.is_none()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        !app.in_flight
            && app.message_queue.is_empty()
            && app.pending_stream.is_none()
            && app.pending_stream_connect.is_none(),
        "embedded app should finish the turn before quit (in_flight={}, queue_len={}, pending_stream={}, pending_connect={})",
        app.in_flight,
        app.message_queue.len(),
        app.pending_stream.is_some(),
        app.pending_stream_connect.is_some()
    );
    let assistant_blocks = app
        .chat_blocks
        .iter()
        .filter(|block| block.role == ChatRole::Assistant)
        .map(|block| block.content.clone())
        .collect::<Vec<_>>();
    assert!(
        assistant_blocks
            .iter()
            .any(|content| content.contains("embedded-quit-ok")),
        "embedded app should receive assistant output before quit; assistant_blocks={:?}",
        assistant_blocks
    );

    app.input_buffer = "/quit".to_string();
    app.cursor_index = app.input_buffer.chars().count();
    let should_quit = app
        .submit_input()
        .await
        .expect("quit command should succeed");
    assert!(!should_quit);

    let quit_deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while tokio::time::Instant::now() < quit_deadline {
        app.on_tick().await;
        if app.should_exit {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(app.should_exit, "quit should exit the tui loop");

    handle
        .shutdown()
        .await
        .expect("embedded server shutdown should complete");
    server_task
        .await
        .expect("server task should join")
        .expect("server should stop cleanly");

    let memory_dir = workspace_dir.join("memory");
    let mut found_session_entry = false;
    let mut markdown_files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&memory_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            let content =
                std::fs::read_to_string(&path).expect("workspace memory file should be readable");
            markdown_files.push(format!(
                "{} => {}",
                path.display(),
                content.lines().take(8).collect::<Vec<_>>().join(" | ")
            ));
            if content.contains("session:") {
                found_session_entry = true;
                break;
            }
        }
    }

    assert!(
        found_session_entry,
        "embedded quit should flush workspace memory into markdown files; memory_dir={}, files={:?}",
        memory_dir.display(),
        markdown_files
    );
}
