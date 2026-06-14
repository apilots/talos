use reqwest;
use serde::{Deserialize, Serialize};
use tokio;

#[derive(Serialize, Deserialize, Debug)]
struct Context {
    source_type: String,
    provider: String,
    channel_id: String,
    user_id: String,
    thread_id: Option<String>,
    message_id: String,
    trace_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct MessageInputItem {
    #[serde(rename = "type")]
    item_type: String,
    text: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct MessageRequest {
    input: Vec<MessageInputItem>,
    workspace_id: Option<String>,
    context: Option<Context>,
    model: Option<String>,
    project_id: Option<String>,
    current_directory: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing Talos Gateway API with Bailian LLM...");

    if std::env::var("TALOS_LLM_BAILIAN_API_KEY").is_err() {
        println!("Warning: TALOS_LLM_BAILIAN_API_KEY environment variable not set");
    }

    let client = reqwest::Client::new();

    println!("Testing health endpoint...");
    match client
        .get("http://localhost:8080/health")
        .send()
        .await
    {
        Ok(response) => {
            println!("Health check status: {}", response.status());
            if response.status().is_success() {
                println!("✓ Health check passed");
            } else {
                println!("✗ Health check failed");
            }
        }
        Err(e) => {
            println!("✗ Health check failed with error: {}", e);
        }
    }

    // Test native messages endpoint
    println!("\nTesting messages endpoint...");
    let session_id = format!(
        "sess_integration_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let context = Context {
        source_type: "http".to_string(),
        provider: "http".to_string(),
        channel_id: "default".to_string(),
        user_id: "test_user".to_string(),
        thread_id: None,
        message_id: "msg-integration-test".to_string(),
        trace_id: "trace-integration-test".to_string(),
    };

    let message_request = MessageRequest {
        input: vec![MessageInputItem {
            item_type: "text".to_string(),
            text: "hello from integration test".to_string(),
        }],
        workspace_id: None,
        context: Some(context),
        model: None,
        project_id: None,
        current_directory: None,
    };

    match client
        .post(format!(
            "http://localhost:8080/v1/sessions/{}/messages",
            session_id
        ))
        .json(&message_request)
        .send()
        .await
    {
        Ok(response) => {
            println!("Messages API status: {}", response.status());
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(message_response) => {
                        let text = message_response
                            .get("output")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|item| item.get("text"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        println!("✓ Messages API test successful");
                        println!("Response content: {}", text);
                    }
                    Err(e) => println!("✗ Failed to parse messages response: {}", e),
                }
            } else {
                let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
                println!("✗ Messages API failed with error: {}", error_text);
            }
        }
        Err(e) => {
            println!("✗ Messages API request failed with error: {}", e);
        }
    }

    Ok(())
}
