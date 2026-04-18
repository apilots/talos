//! Complete example demonstrating the enhanced tool system with security policies and network tools

use serde_json::json;
use talos_tools::{
    BashTool, ReadTool, ToolContext, ToolDispatchOutcome, ToolRegistry, WebFetchTool,
    WebSearchTool, WriteTool,
};
use talos_tools_policy::ToolPolicy;

async fn dispatch(
    registry: &ToolRegistry,
    call_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<ToolDispatchOutcome, Box<dyn std::error::Error>> {
    let invocation = registry.build_function_invocation(call_id, tool_name, arguments)?;
    Ok(registry
        .execute_invocation_with_context(invocation, ToolContext::default())
        .await?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Complete Enhanced Tool System Demo ===\n");

    // 1. Setup a policy that allows most tools but restricts Bash
    println!("1. Setting up security policy...");
    let policy = ToolPolicy::allow_list(vec!["Read", "Write", "WebSearch", "WebFetch"]);
    let registry = ToolRegistry::new_with_policy(policy);

    // Register all available tools
    registry.register(std::sync::Arc::new(ReadTool));
    registry.register(std::sync::Arc::new(WriteTool));
    registry.register(std::sync::Arc::new(BashTool));
    registry.register(std::sync::Arc::new(WebSearchTool));
    registry.register(std::sync::Arc::new(WebFetchTool));

    println!("   Available tools: {:?}", registry.list_tool_names());
    println!("   Allowed tools: {:?}", registry.list_allowed_tool_names());
    println!();

    // 2. Demonstrate Web Search Tool
    println!("2. Web Search Tool:");
    let search_result = dispatch(
        &registry,
        "call-web-search",
        "WebSearch",
        json!({"query": "Rust programming language"}),
    )
    .await;
    match search_result {
        Ok(ToolDispatchOutcome::Executed { payload }) => {
            println!(
                "   Search result: {}",
                payload.content().lines().next().unwrap_or("No output")
            );
        }
        Ok(ToolDispatchOutcome::Pending { tool_name, .. }) => {
            println!(
                "   Search entered pending state unexpectedly: {}",
                tool_name
            );
        }
        Err(e) => println!("   Search failed: {}", e),
    }
    println!();

    // 3. Demonstrate Web Fetch Tool
    println!("3. Web Fetch Tool:");
    let fetch_result = dispatch(
        &registry,
        "call-web-fetch",
        "WebFetch",
        json!({"url": "https://httpbin.org/html"}),
    )
    .await;
    match fetch_result {
        Ok(ToolDispatchOutcome::Executed { payload }) => {
            println!(
                "   Fetch result: {}",
                payload.content().lines().next().unwrap_or("No output")
            );
        }
        Ok(ToolDispatchOutcome::Pending { tool_name, .. }) => {
            println!("   Fetch entered pending state unexpectedly: {}", tool_name);
        }
        Err(e) => println!("   Fetch failed: {}", e),
    }
    println!();

    // 4. Demonstrate Security Policy Enforcement
    println!("4. Security Policy Enforcement:");
    println!("   Attempting to execute 'Bash' tool (should be blocked by policy)...");

    let exec_result = dispatch(
        &registry,
        "call-bash-blocked",
        "Bash",
        json!({"command": "ls -la"}),
    )
    .await;
    match exec_result {
        Ok(_) => println!("   Bash tool executed successfully (unexpected!)"),
        Err(e) => println!("   Bash tool execution blocked: {} (as expected)", e),
    }
    println!();

    // 5. Show how to update policy dynamically
    println!("5. Dynamic Policy Update:");
    println!(
        "   Current allowed tools: {:?}",
        registry.list_allowed_tool_names()
    );

    let new_policy = ToolPolicy::allow_list(vec!["Read", "Write", "Bash", "WebSearch", "WebFetch"]);
    let new_registry = ToolRegistry::new_with_policy(new_policy);

    new_registry.register(std::sync::Arc::new(ReadTool));
    new_registry.register(std::sync::Arc::new(WriteTool));
    new_registry.register(std::sync::Arc::new(BashTool));
    new_registry.register(std::sync::Arc::new(WebSearchTool));
    new_registry.register(std::sync::Arc::new(WebFetchTool));

    println!(
        "   After policy update, allowed tools: {:?}",
        new_registry.list_allowed_tool_names()
    );
    println!();

    // 6. Now Bash should work with the updated policy
    println!("6. Verifying Updated Policy:");
    let exec_result_after_update = dispatch(
        &new_registry,
        "call-bash-allowed",
        "Bash",
        json!({"command": "echo 'Policy updated successfully'"}),
    )
    .await;
    match exec_result_after_update {
        Ok(ToolDispatchOutcome::Executed { payload }) => {
            println!("   Bash tool now works: {}", payload.content().trim())
        }
        Ok(ToolDispatchOutcome::Pending { tool_name, .. }) => {
            println!("   Bash unexpectedly entered pending state: {}", tool_name)
        }
        Err(e) => println!("   Bash tool still blocked: {}", e),
    }
    println!();

    // 7. Demonstrate tool availability checking
    println!("7. Tool Availability Checking:");
    println!(
        "   Is 'WebSearch' available? {}",
        new_registry.is_tool_available("WebSearch")
    );
    println!(
        "   Is 'Bash' available? {}",
        new_registry.is_tool_available("Bash")
    );
    println!(
        "   Is 'nonexistent' available? {}",
        new_registry.is_tool_available("nonexistent")
    );
    println!();

    println!("=== Complete Enhanced Tool System Demo Finished ===");

    Ok(())
}
