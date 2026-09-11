use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use talos_types::{
    McpCallToolRequest, McpServerDefinition, McpServerRuntimeDescriptor, McpServerSource,
    McpServerStatus, McpTransportDefinition,
};

use crate::{McpConnection, TalosMcpRuntime};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "talos-mcp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn descriptor(&self, name: &str, mode: &str) -> McpServerRuntimeDescriptor {
        McpServerRuntimeDescriptor {
            content_revision: None,
            server: McpServerDefinition {
                credentials: None,
                name: name.into(),
                display_name: None,
                description: None,
                transport: McpTransportDefinition::Stdio {
                    command: "/usr/bin/python3".into(),
                    args: vec![
                        format!("{}/tests/fixtures/server.py", env!("CARGO_MANIFEST_DIR")),
                        self.0.join(name).to_str().unwrap().into(),
                        mode.into(),
                    ],
                    env: BTreeMap::new(),
                    cwd: None,
                },
            },
            source: McpServerSource::Config,
        }
    }
    fn marker(&self, name: &str, suffix: &str) -> PathBuf {
        self.0.join(format!("{name}.{suffix}"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Also clean fixture groups if an intentional ablation panics before the
        // async runtime can finish its normal shutdown. Successful tests assert
        // process disappearance before this fallback runs.
        for entry in std::fs::read_dir(&self.0).unwrap().flatten() {
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "pid")
            {
                if let Ok(pid) = std::fs::read_to_string(entry.path())
                    .unwrap()
                    .parse::<i32>()
                {
                    unsafe {
                        libc::kill(-pid, libc::SIGKILL);
                    }
                }
            }
        }
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

async fn until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture condition should become observable");
}
async fn call(
    connection: &McpConnection,
    tool: &str,
    arguments: serde_json::Value,
) -> talos_error::TalosResult<talos_types::McpCallToolResponse> {
    connection
        .call_tool(
            McpCallToolRequest {
                server_name: connection.name().into(),
                tool_name: tool.into(),
                arguments,
            },
            std::future::pending(),
        )
        .await
}
fn gone(pid: u32) -> bool {
    // Signal 0 only probes process existence; it never delivers a signal.
    unsafe {
        libc::kill(pid as i32, 0) == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
}

#[tokio::test]
async fn background_start_reuse_replacement_and_shutdown_are_isolated() {
    let fixture = Fixture::new();
    let runtime = TalosMcpRuntime::new();
    let healthy = fixture.descriptor("healthy", "normal");
    let hung = fixture.descriptor("hung", "hung");
    let mut broken = fixture.descriptor("broken", "normal");
    if let McpTransportDefinition::Stdio { command, .. } = &mut broken.server.transport {
        *command = "/missing/talos-mcp-fixture".into();
    }
    let desired = vec![
        healthy.clone(),
        hung,
        broken,
        fixture.descriptor("oversized", "oversized"),
    ];
    // Rejected identities must not publish a partial replacement or yield unusable cursors.
    for name in [String::new(), "x".repeat(257), "ambiguous__server".into()] {
        let mut invalid = healthy.clone();
        invalid.server.name = name;
        assert_eq!(
            runtime
                .prepare((vec![invalid]).into_iter().map(Into::into).collect())
                .err()
                .unwrap()
                .error_code(),
            talos_error::ErrorCode::InvalidName
        );
    }
    runtime
        .prepare((desired.clone()).into_iter().map(Into::into).collect())
        .unwrap()
        .publish();
    until(|| runtime.connection("healthy").is_ok() && fixture.marker("hung", "pid").exists()).await;
    until(|| {
        runtime
            .list_servers(Default::default())
            .unwrap()
            .data
            .iter()
            .any(|snapshot| {
                snapshot.name == "oversized"
                    && matches!(snapshot.status, McpServerStatus::Failed { .. })
            })
    })
    .await;
    let old = runtime.connection("healthy").unwrap();
    assert_eq!(
        old.tools()
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["inspect", "hold"]
    );
    let first = call(&old, "inspect", serde_json::json!({}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    let old_pid = first["pid"].as_u64().unwrap() as u32;
    let hung_pid = std::fs::read_to_string(fixture.marker("hung", "pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    assert!(runtime
        .list_servers(Default::default())
        .unwrap()
        .data
        .iter()
        .any(|snapshot| snapshot.name == "broken"
            && matches!(snapshot.status, McpServerStatus::Failed { .. })));
    runtime
        .prepare((desired).into_iter().map(Into::into).collect())
        .unwrap()
        .publish();
    assert_eq!(
        call(&old, "inspect", serde_json::json!({}))
            .await
            .unwrap()
            .structured_content
            .unwrap(),
        first
    );
    let mut replacement = healthy;
    if let McpTransportDefinition::Stdio { env, .. } = &mut replacement.server.transport {
        env.insert("REVISION".into(), "2".into());
    }
    runtime
        .prepare((vec![replacement]).into_iter().map(Into::into).collect())
        .unwrap()
        .publish();
    assert!(call(&old, "inspect", serde_json::json!({})).await.is_err());
    until(|| runtime.connection("healthy").is_ok()).await;
    let current = runtime.connection("healthy").unwrap();
    let second = call(&current, "inspect", serde_json::json!({}))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(second["revision"], "2");
    assert_ne!(first["pid"], second["pid"]);
    let page = current
        .list_resources(None, std::future::pending())
        .await
        .unwrap();
    assert_eq!(page.data[0].uri, "fixture:first");
    let page = current
        .list_resources(page.next_cursor, std::future::pending())
        .await
        .unwrap();
    assert_eq!(page.data[0].uri, "fixture:last");
    assert!(page.next_cursor.is_none());
    call(&current, "inspect", serde_json::json!({"refresh": true}))
        .await
        .unwrap();
    until(|| {
        runtime
            .connection("healthy")
            .unwrap()
            .tools()
            .iter()
            .any(|tool| tool.name == "changed")
    })
    .await;
    assert!(call(&current, "inspect", serde_json::json!({}))
        .await
        .is_err());
    let current = runtime.connection("healthy").unwrap();
    call(&current, "inspect", serde_json::json!({}))
        .await
        .unwrap();
    runtime.shutdown().await.unwrap();
    until(|| gone(old_pid) && gone(hung_pid) && gone(second["pid"].as_u64().unwrap() as u32)).await;
}

#[tokio::test]
async fn retirement_drains_admitted_calls_and_root_exit_interrupts_discovery() {
    let fixture = Fixture::new();
    let runtime = TalosMcpRuntime::new();
    runtime
        .prepare(
            (vec![
                fixture.descriptor("drain", "normal"),
                fixture.descriptor("exit", "normal"),
            ])
            .into_iter()
            .map(Into::into)
            .collect(),
        )
        .unwrap()
        .publish();
    until(|| runtime.connections().len() == 2).await;
    let connection = runtime.connection("drain").unwrap();
    let active = connection.clone();
    let call_task = tokio::spawn(async move { call(&active, "hold", serde_json::json!({})).await });
    until(|| fixture.marker("drain", "holding").exists()).await;
    runtime
        .prepare(
            (vec![fixture.descriptor("exit", "normal")])
                .into_iter()
                .map(Into::into)
                .collect(),
        )
        .unwrap()
        .publish();
    assert!(call(&connection, "inspect", serde_json::json!({}))
        .await
        .is_err());
    std::fs::write(fixture.marker("drain", "release"), "").unwrap();
    assert!(call_task.await.unwrap().is_ok());
    let response = call(
        &runtime.connection("exit").unwrap(),
        "inspect",
        serde_json::json!({"refresh_exit": true}),
    )
    .await
    .unwrap();
    let child_pid = response.structured_content.unwrap()["child"]
        .as_u64()
        .unwrap() as u32;
    until(|| {
        runtime
            .list_servers(Default::default())
            .unwrap()
            .data
            .iter()
            .any(|snapshot| matches!(snapshot.status, McpServerStatus::Exited { .. }))
    })
    .await;
    assert!(runtime.connection("exit").is_err());
    runtime.shutdown().await.unwrap();
    until(|| gone(child_pid)).await;
}
