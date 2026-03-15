use std::sync::Arc;
use talos_agent::{Agent, AgentImpl};
use talos_client::{ClientConfig, TalosClient};
use talos_error::TalosResult;
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
use talos_tools::{ReadTool, ToolRegistry, WebSearchTool, WriteTool};
use talos_tools_policy::ToolPolicy;
use talos_ui_tui::{run_tui_app, run_tui_app_with_client};

const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:8080";
const DEFAULT_GATEWAY_TOKEN: &str = "talos-local-test-token";

/// Simple mock LLM provider for demo purposes
struct DemoLlmProvider;

#[async_trait::async_trait]
impl LlmProvider for DemoLlmProvider {
    fn name(&self) -> &str {
        "demo"
    }

    fn default_model(&self) -> &str {
        "demo-model"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: "Hello! This is a response from the demo LLM provider.".to_string(),
            model: "demo-model".to_string(),
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
            },
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        drop(tx); // Close the sender immediately for mock
        Ok(ChatStream { rx })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting Talos TUI application...");

    let args: Vec<String> = std::env::args().collect();
    let use_gateway = has_flag(&args, "--gateway") || has_flag(&args, "-g");
    let gateway_url = option_arg(&args, "--gateway-url")
        .or_else(|| std::env::var("TALOS_GATEWAY_URL").ok())
        .unwrap_or_else(|| DEFAULT_GATEWAY_URL.to_string());
    let auth_token = option_arg(&args, "--auth-token")
        .or_else(|| std::env::var("GATEWAY_TOKEN").ok())
        .or_else(|| Some(DEFAULT_GATEWAY_TOKEN.to_string()));

    if use_gateway {
        println!("Starting in gateway client mode...");
        println!("Gateway URL: {}", gateway_url);

        let config = ClientConfig {
            gateway_url,
            auth_token,
            timeout: 30,
        };

        let client = Arc::new(TalosClient::new(config)?);
        run_tui_app_with_client(client).await?;
    } else {
        println!("Starting in direct agent mode...");

        let llm_provider = Arc::new(DemoLlmProvider {});
        let policy = ToolPolicy::allow_list(vec!["web_search", "read", "write"]);
        let tool_registry = Arc::new(ToolRegistry::new_with_policy(policy));
        tool_registry.register(Arc::new(WebSearchTool));
        tool_registry.register(Arc::new(ReadTool));
        tool_registry.register(Arc::new(WriteTool));

        let agent_impl = AgentImpl::new(
            "tui_demo_agent".to_string(),
            "TUI Demo Agent".to_string(),
            "An agent for TUI demonstration".to_string(),
            llm_provider,
            "demo-model".to_string(),
            tool_registry,
            Default::default(),
        );
        let agent: Arc<dyn Agent> = Arc::new(agent_impl);
        run_tui_app(agent).await?;
    }

    Ok(())
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn option_arg(args: &[String], key: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == key {
            return iter.next().cloned();
        }
        if let Some((prefix, value)) = arg.split_once('=') {
            if prefix == key && !value.trim().is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
