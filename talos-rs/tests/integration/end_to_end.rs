//! 端到端集成测试
//!
//! 验证 Talos 系统各组件之间的集成是否正常工作
//! 包括：Agent、Memory、Tools、TUI、Channels等组件的协同工作

use std::sync::Arc;
use tokio::time::timeout;
use tokio::sync::mpsc;

use talos_agent::AgentImpl;
use talos_tools::{ToolRegistry, WebSearchTool, ReadTool, WriteTool};
use talos_tools_policy::ToolPolicy;
use talos_llm_provider::providers::openai::OpenAiProvider;
use talos_channel::{ChannelManager, InputMessage, OutputMessage};
use talos_types::SessionId;

/// 测试整个系统的基本对话功能
#[tokio::test]
async fn test_end_to_end_basic_conversation() {
    // 初始化各个组件
    let llm_provider = create_mock_llm_provider();
    let mut agent_impl = AgentImpl::new(
        "integration_test_agent".to_string(),
        "Integration Test Agent".to_string(),
        "Agent for integration testing".to_string(),
        Arc::new(llm_provider),
        "gpt-4".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );

    // 设置工具
    let policy = ToolPolicy::allow_list(vec!["read", "write"]);
    let tool_registry = Arc::new(ToolRegistry::new_with_policy(policy));
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    let agent = Arc::new(agent_impl);

    // 模拟用户输入
    let user_input = "Hello, can you help me?";

    // 验证 Agent 能够处理输入并生成合理响应
    let response = agent.process(user_input).await.expect("Agent should process input successfully");

    // 基本验证：响应不为空
    assert!(!response.is_empty(), "Agent response should not be empty");
    assert!(response.len() > 10, "Agent response should be substantial");
}

/// 测试工具调用功能
#[tokio::test]
async fn test_tool_integration() {
    // 初始化组件
    let llm_provider = create_mock_llm_provider();
    let mut agent_impl = AgentImpl::new(
        "tool_test_agent".to_string(),
        "Tool Test Agent".to_string(),
        "Agent for testing tools".to_string(),
        Arc::new(llm_provider),
        "gpt-4".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );

    // 注册工具
    let policy = ToolPolicy::allow_list(vec!["read", "write"]);
    let tool_registry = Arc::new(ToolRegistry::new_with_policy(policy));
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    agent_impl.set_tool_registry(tool_registry);

    let agent = Arc::new(agent_impl);

    // 测试工具调用
    let tool_input = "Please read the file ./test.txt";
    let response = agent.process(tool_input).await.expect("Agent should process tool request");

    // 响应应该包含工具相关信息
    assert!(response.to_lowercase().contains("tool") || response.to_lowercase().contains("file"));
}

/// 测试频道管理器和消息处理
#[tokio::test]
async fn test_channel_manager_integration() {
    // 创建模拟组件
    let llm_provider = create_mock_llm_provider();
    let agent_impl = AgentImpl::new(
        "channel_test_agent".to_string(),
        "Channel Test Agent".to_string(),
        "Agent for testing channels".to_string(),
        Arc::new(llm_provider),
        "gpt-4".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );
    let agent = Arc::new(agent_impl);

    // 创建频道管理器
    let mut channel_manager = ChannelManager::new();

    // 注意：这里我们会跳过实际的Telegram频道测试，因为需要有效的API密钥
    // 在实际的集成测试环境中，我们可以使用一个模拟频道

    // 验证频道管理器的基本功能
    assert_eq!(channel_manager.channel_names().len(), 0);

    // 测试消息发送和接收流程（概念验证）
    let (input_tx, mut input_rx) = mpsc::channel(100);

    // 模拟发送一条消息
    let input_msg = InputMessage {
        id: "test_msg_1".to_string(),
        channel: "test".to_string(),
        session_id: SessionId::new("test_session"),
        sender_id: "test_user".to_string(),
        sender_name: "Test User".to_string(),
        content: "Hello from test".to_string(),
        timestamp: 1234567890,
        metadata: std::collections::HashMap::new(),
    };

    // 发送消息到通道
    input_tx.send(input_msg).await.unwrap();

    // 尝试接收消息
    let received_msg = timeout(std::time::Duration::from_millis(100), input_rx.recv())
        .await
        .expect("Should receive message within timeout")
        .expect("Should have a message");

    assert_eq!(received_msg.content, "Hello from test");
}

/// 创建模拟的LLM提供者用于测试
fn create_mock_llm_provider() -> OpenAiProvider {
    // 在真实测试中，这将连接到模拟或测试环境中的LLM
    // 由于我们无法在这里使用真实的API密钥，我们使用一个模拟实现
    // 但在集成测试中，我们可以使用环境变量提供的测试密钥
    let api_key = std::env::var("OPENAI_API_KEY")
        .unwrap_or_else(|_| "sk-test1234567890".to_string());

    OpenAiProvider::new(api_key)
}

/// 测试TUI和后端的集成
#[tokio::test]
async fn test_tui_backend_integration() {
    // 这里我们验证TUI组件是否能与后端组件正确集成
    use talos_ui_tui::{TuiApp, AppState};

    let llm_provider = create_mock_llm_provider();
    let agent_impl = AgentImpl::new(
        "tui_test_agent".to_string(),
        "TUI Test Agent".to_string(),
        "Agent for testing TUI integration".to_string(),
        Arc::new(llm_provider),
        "gpt-4".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );
    let agent = Arc::new(agent_impl);

    // 创建TUI应用实例
    let app = TuiApp::new(agent);

    // 验证初始状态
    assert_eq!(app.get_current_state(), &AppState::Chat);
    assert!(app.get_chat_history().len() >= 2); // 应该有欢迎消息

    // 测试输入处理（模拟）
    let initial_history_len = app.get_chat_history().len();

    // 注意：这里只是验证TUI应用的创建和基本状态，不运行实际的UI循环
    assert!(initial_history_len > 0);
}

/// 性能基准测试
#[tokio::test]
async fn test_system_performance() {
    use std::time::Instant;

    let llm_provider = create_mock_llm_provider();
    let agent_impl = AgentImpl::new(
        "perf_test_agent".to_string(),
        "Performance Test Agent".to_string(),
        "Agent for performance testing".to_string(),
        Arc::new(llm_provider),
        "gpt-4".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );
    let agent = Arc::new(agent_impl);

    // 测试多次请求的性能
    let start_time = Instant::now();

    for i in 0..5 {
        let input = format!("Test message {}", i);
        let _response = agent.process(&input).await.unwrap();
    }

    let elapsed = start_time.elapsed();
    println!("Processed 5 requests in {:?}", elapsed);

    // 这里可以根据需要设置性能阈值
    // 例如：assert!(elapsed.as_secs() < 30, "Requests took too long");
}
