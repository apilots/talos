//! Demo example showcasing the Agent with enhanced tool system

use std::sync::Arc;
use talos_agent::{Agent, AgentImpl};
use talos_tools::{
    ExecTool, HttpRequestTool, ReadTool, ToolRegistry, WebFetchTool, WebSearchTool, WriteTool,
};
use talos_tools_policy::ToolPolicy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Agent with Enhanced Tool System Demo ===\n");

    // For this demo, we'll use a mock LLM provider from the agent crate
    use talos_agent::MockAgent;

    // Create a mock agent instance to focus on tool integration
    let agent = MockAgent::new();

    // This demo focuses on demonstrating the agent's interface and tool integration
    // In a real implementation, you would create a real agent with a proper LLM provider

    println!("1. Mock agent created successfully");

    // We can't test the actual agent implementation with tools without a real LLM provider
    // because the Agent trait implementation has async methods that need proper implementation

    println!("2. This demo shows that the agent crate compiles correctly with all its components");
    println!("3. The agent has been enhanced to support:");
    println!("   - Tool call processing with process_tool_calls()");
    println!("   - System prompt generation with available tools");
    println!("   - Tool execution result handling and memory storage");
    println!("   - Better integration between agent and tool registry");

    // Demonstrate the tool registry with security policies
    let policy = ToolPolicy::allow_list(vec!["web_search", "web_fetch", "read", "write"]);
    let registry = Arc::new(ToolRegistry::new_with_policy(policy));

    // Register tools
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(ExecTool));
    registry.register(Arc::new(WebSearchTool));
    registry.register(Arc::new(WebFetchTool));
    registry.register(Arc::new(HttpRequestTool));

    println!("4. Tool registry created with policy:");
    println!("   Available tools: {:?}", registry.list_tool_names());
    println!("   Allowed tools: {:?}", registry.list_allowed_tool_names());

    // Test tool execution
    println!("\n5. Testing tool execution:");
    match registry
        .execute("web_search", serde_json::json!({"query": "test"}))
        .await
    {
        Ok(result) => {
            println!("   Web search tool executed: {}", result.success);
            println!(
                "   Result: {}",
                result.output.lines().next().unwrap_or("No output")
            );
        }
        Err(e) => println!("   Web search tool failed: {}", e),
    }

    // Test security policy
    println!("\n6. Testing security policy (exec should be blocked):");
    match registry
        .execute("exec", serde_json::json!({"command": "ls"}))
        .await
    {
        Ok(_) => println!("   Exec tool executed (unexpected)"),
        Err(e) => println!("   Exec tool blocked by policy: {} (expected)", e),
    }

    println!("\n=== Agent Tool Integration Demo Completed ===");

    Ok(())
}
