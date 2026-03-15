//! Complete example demonstrating the enhanced tool system with security policies and network tools

use serde_json::json;
use talos_tools::{
    ExecTool, HttpRequestTool, ReadTool, ToolRegistry, WebFetchTool, WebSearchTool, WriteTool,
};
use talos_tools_policy::ToolPolicy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Complete Enhanced Tool System Demo ===\n");

    // 1. Setup a policy that allows most tools but restricts exec
    println!("1. Setting up security policy...");
    let policy = ToolPolicy::allow_list(vec![
        "read",
        "write",
        "web_search",
        "web_fetch",
        "http_request",
    ]);
    let registry = ToolRegistry::new_with_policy(policy);

    // Register all available tools
    registry.register(std::sync::Arc::new(ReadTool));
    registry.register(std::sync::Arc::new(WriteTool));
    registry.register(std::sync::Arc::new(ExecTool));
    registry.register(std::sync::Arc::new(WebSearchTool));
    registry.register(std::sync::Arc::new(WebFetchTool));
    registry.register(std::sync::Arc::new(HttpRequestTool));

    println!("   Available tools: {:?}", registry.list_tool_names());
    println!("   Allowed tools: {:?}", registry.list_allowed_tool_names());
    println!();

    // 2. Demonstrate Web Search Tool
    println!("2. Web Search Tool:");
    let search_result = registry
        .execute("web_search", json!({"query": "Rust programming language"}))
        .await;
    match search_result {
        Ok(output) => {
            println!(
                "   Search result: {}",
                output.output.lines().next().unwrap_or("No output")
            );
        }
        Err(e) => println!("   Search failed: {}", e),
    }
    println!();

    // 3. Demonstrate Web Fetch Tool
    println!("3. Web Fetch Tool:");
    let fetch_result = registry
        .execute("web_fetch", json!({"url": "https://httpbin.org/html"}))
        .await;
    match fetch_result {
        Ok(output) => {
            println!(
                "   Fetch result: {}",
                output.output.lines().next().unwrap_or("No output")
            );
        }
        Err(e) => println!("   Fetch failed: {}", e),
    }
    println!();

    // 4. Demonstrate HTTP Request Tool
    println!("4. HTTP Request Tool:");
    let http_result = registry
        .execute(
            "http_request",
            json!({
                "url": "https://httpbin.org/get",
                "method": "GET"
            }),
        )
        .await;
    match http_result {
        Ok(output) => {
            println!(
                "   HTTP result: {}",
                output.output.lines().next().unwrap_or("No output")
            );
        }
        Err(e) => println!("   HTTP request failed: {}", e),
    }
    println!();

    // 5. Demonstrate Security Policy Enforcement
    println!("5. Security Policy Enforcement:");
    println!("   Attempting to execute 'exec' tool (should be blocked by policy)...");

    let exec_result = registry.execute("exec", json!({"command": "ls -la"})).await;
    match exec_result {
        Ok(_) => println!("   Exec tool executed successfully (unexpected!)"),
        Err(e) => println!("   Exec tool execution blocked: {} (as expected)", e),
    }
    println!();

    // 6. Show how to update policy dynamically
    println!("6. Dynamic Policy Update:");
    println!(
        "   Current allowed tools: {:?}",
        registry.list_allowed_tool_names()
    );

    // Since we can't change the registry itself, let's create a new one with updated policy
    let new_policy = ToolPolicy::allow_list(vec![
        "read",
        "write",
        "exec",
        "web_search",
        "web_fetch",
        "http_request",
    ]);
    let new_registry = ToolRegistry::new_with_policy(new_policy);

    // Register the same tools with the new registry
    new_registry.register(std::sync::Arc::new(ReadTool));
    new_registry.register(std::sync::Arc::new(WriteTool));
    new_registry.register(std::sync::Arc::new(ExecTool));
    new_registry.register(std::sync::Arc::new(WebSearchTool));
    new_registry.register(std::sync::Arc::new(WebFetchTool));
    new_registry.register(std::sync::Arc::new(HttpRequestTool));

    println!(
        "   After policy update, allowed tools: {:?}",
        new_registry.list_allowed_tool_names()
    );
    println!();

    // 7. Now exec should work with the updated policy
    println!("7. Verifying Updated Policy:");
    let exec_result_after_update = new_registry
        .execute(
            "exec",
            json!({"command": "echo 'Policy updated successfully'"}),
        )
        .await;
    match exec_result_after_update {
        Ok(output) => println!("   Exec tool now works: {}", output.output.trim()),
        Err(e) => println!("   Exec tool still blocked: {}", e),
    }
    println!();

    // 8. Demonstrate tool availability checking
    println!("8. Tool Availability Checking:");
    println!(
        "   Is 'web_search' available? {}",
        new_registry.is_tool_available("web_search")
    );
    println!(
        "   Is 'exec' available? {}",
        new_registry.is_tool_available("exec")
    );
    println!(
        "   Is 'nonexistent' available? {}",
        new_registry.is_tool_available("nonexistent")
    );
    println!();

    println!("=== Complete Enhanced Tool System Demo Finished ===");

    Ok(())
}
