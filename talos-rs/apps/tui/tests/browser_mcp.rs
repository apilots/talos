use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

#[tokio::test]
async fn bundled_browser_negotiates_stdio_before_cli_authentication_or_runtime_startup() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("invalid.yaml");
    std::fs::write(&config, "invalid: [").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_talos"))
        .arg("--browser-mcp")
        .env_clear()
        .env("TALOS_HOME", home.path())
        .env("TALOS_CONFIG", &config)
        .current_dir(home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26", "capabilities": {},
            "clientInfo": {"name": "talos-tui-entrypoint-test", "version": "1"}
        }
    });
    input
        .write_all(format!("{initialize}\n").as_bytes())
        .await
        .unwrap();
    let line = timeout(Duration::from_secs(10), output.next_line())
        .await
        .expect("Browser MCP initialize must finish without login or model access")
        .unwrap()
        .expect("Browser MCP must answer initialize on stdout");
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["serverInfo"]["name"], "talos-browser");
    assert_eq!(response["result"]["protocolVersion"], "2025-03-26");
    assert!(response["result"]["capabilities"]["tools"].is_object());
    assert!(response.get("error").is_none(), "{response}");

    input
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\
              {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
        )
        .await
        .unwrap();
    let line = timeout(Duration::from_secs(10), output.next_line())
        .await
        .expect("Browser MCP must serve requests after initialization")
        .unwrap()
        .expect("Browser MCP must return its tool catalog");
    let response: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(response["id"], 2);
    assert!(response.get("error").is_none(), "{response}");
    assert!(response["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "browser_state"));

    drop(input);
    let status = timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("Browser MCP must exit when its stdio client disconnects")
        .unwrap();
    assert!(status.success(), "Browser MCP exited with {status}");
    assert!(!home.path().join("logs").exists());
    assert!(!home.path().join("data").exists());
}
