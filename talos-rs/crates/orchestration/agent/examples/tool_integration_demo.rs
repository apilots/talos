//! Demo example showcasing the Agent with enhanced tool system

use std::sync::Arc;
use talos_agent::MockAgent;
use talos_tools::{
    BashTool, ReadTool, ToolContext, ToolDispatchOutcome, ToolRegistry, WebFetchTool,
    WebSearchTool, WriteTool,
};
use talos_tools_policy::ToolPolicy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Agent with Enhanced Tool System Demo ===\n");

    // Create a mock agent instance to focus on tool integration
    let _agent = MockAgent::new();

    // This demo focuses on demonstrating orchestration and tool registry integration.
    // In a real implementation, you would create a real agent with a proper LLM provider

    println!("1. Mock agent created successfully");

    // We can't test the actual agent implementation with tools without a real LLM provider.

    println!("2. This demo shows that the agent crate compiles correctly with all its components");
    println!("3. The agent has been enhanced to support:");
    println!("   - System prompt generation with available tools");
    println!("   - Tool execution result handling and memory storage");
    println!("   - Better integration between agent and tool registry");

    // Demonstrate the tool registry with security policies
    let policy = ToolPolicy::allow_list(vec!["WebSearch", "WebFetch", "Read", "Write"]);
    let registry = Arc::new(ToolRegistry::new_with_policy(policy));

    // Register tools
    registry.register(Arc::new(ReadTool));
    registry.register(Arc::new(WriteTool));
    registry.register(Arc::new(BashTool));
    registry.register(Arc::new(WebSearchTool));
    registry.register(Arc::new(WebFetchTool));

    println!("4. Tool registry created with policy:");
    println!("   Available tools: {:?}", registry.list_tool_names());
    println!("   Allowed tools: {:?}", registry.list_allowed_tool_names());

    // Test tool execution
    println!("\n5. Testing tool execution:");
    let web_search_invocation = registry.build_function_invocation(
        "call-web-search",
        "WebSearch",
        serde_json::json!({"query": "test"}),
    )?;
    match registry
        .execute_invocation_with_context(web_search_invocation, ToolContext::default())
        .await
    {
        Ok(ToolDispatchOutcome::Executed { payload }) => {
            println!("   Web search tool executed: {}", !payload.is_error());
            println!(
                "   Result: {}",
                payload.content().lines().next().unwrap_or("No output")
            );
        }
        Ok(ToolDispatchOutcome::Pending { tool_name, .. }) => {
            println!("   Tool unexpectedly entered pending state: {}", tool_name);
        }
        Err(e) => println!("   Web search tool failed: {}", e),
    }

    // Test security policy
    println!("\n6. Testing security policy (Bash should be blocked):");
    let exec_invocation = registry.build_function_invocation(
        "call-bash",
        "Bash",
        serde_json::json!({"command": "ls"}),
    )?;
    match registry
        .execute_invocation_with_context(exec_invocation, ToolContext::default())
        .await
    {
        Ok(_) => println!("   Bash tool executed (unexpected)"),
        Err(e) => println!("   Bash tool blocked by policy: {} (expected)", e),
    }

    println!("\n=== Agent Tool Integration Demo Completed ===");

    Ok(())
}
