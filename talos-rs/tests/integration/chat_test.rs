//! 集成测试 - 聊天功能
//! 
//! 使用百炼 API (兼容 OpenAI 协议) 进行端到端测试
//! 
//! 运行方式：
//! ```bash
//! export BAILIAN_API_KEY="sk-xxx"
//! cargo test --test integration -- --nocapture
//! ```

use talos_llm_provider::{LlmProvider, ChatRequest, ChatMessage};
use talos_llm_openai::OpenAIProvider;

const BAILIAN_BASE_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";

fn create_bailian_provider() -> OpenAIProvider {
    use talos_llm_openai::OpenAIConfig;
    
    let api_key = std::env::var("BAILIAN_API_KEY")
        .expect("BAILIAN_API_KEY must be set");
    
    let config = OpenAIConfig::new(api_key).with_base_url(BAILIAN_BASE_URL.to_string());
    
    OpenAIProvider::new(config)
}

#[tokio::test]
async fn test_bailian_chat_completion() {
    let provider = create_bailian_provider();
    
    let request = ChatRequest::new_user(
        "你好，请用一句话介绍你自己",
        "qwen-plus"
    );
    
    let response = provider.chat(request).await.expect("Chat request failed");
    
    println!("Response: {}", response.content);
    
    assert!(!response.content.is_empty(), "Response should not be empty");
}

#[tokio::test]
async fn test_bailian_multi_turn_conversation() {
    let provider = create_bailian_provider();
    
    // 第一轮对话
    let request1 = ChatRequest {
        messages: vec![ChatMessage::user("什么是 Rust 编程语言？")],
        model: "qwen-plus".to_string(),
        temperature: Some(0.7),
        max_tokens: Some(500),
        system: None,
        stop: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
    };
    
    let response1 = provider.chat(request1).await.expect("First chat failed");
    println!("第一轮回答：{}", response1.content);
    
    // 第二轮对话（带上下文）
    let request2 = ChatRequest {
        messages: vec![
            ChatMessage::user("什么是 Rust 编程语言？"),
            ChatMessage::assistant(response1.content),
            ChatMessage::user("它和 Go 有什么区别？"),
        ],
        model: "qwen-plus".to_string(),
        temperature: Some(0.7),
        max_tokens: Some(500),
        system: None,
        stop: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
    };
    
    let response2 = provider.chat(request2).await.expect("Second chat failed");
    println!("第二轮回答：{}", response2.content);
    
    assert!(!response2.content.is_empty(), "Second response should not be empty");
}

#[tokio::test]
async fn test_bailian_with_system_prompt() {
    let provider = create_bailian_provider();
    
    let request = ChatRequest {
        messages: vec![
            ChatMessage::system("你是一个专业的软件架构师，擅长系统设计。请用简洁的语言回答。"),
            ChatMessage::user("如何设计一个高可用的分布式系统？"),
        ],
        model: "qwen-plus".to_string(),
        temperature: Some(0.5),
        max_tokens: Some(1000),
        system: None,
        stop: None,
        top_p: None,
        tools: None,
        tool_choice: None,
        parallel_tool_calls: None,
    };
    
    let response = provider.chat(request).await.expect("Chat with system prompt failed");
    
    println!("System prompt response: {}", response.content);
    assert!(!response.content.is_empty());
}
