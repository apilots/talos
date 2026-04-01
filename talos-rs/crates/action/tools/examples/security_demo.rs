//! Example demonstrating enhanced tool system with security policies

use serde_json::json;
use talos_tools::{ExecCommandTool, ReadFileTool, ToolRegistry, WriteTool};
use talos_tools_policy::ToolPolicy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Enhanced Tool System with Security Policies ===\n");

    // Example 1: Default policy with built-in tools
    println!("1. Default Policy (Allow List) Example:");
    let registry = ToolRegistry::new();

    registry.register(std::sync::Arc::new(ReadFileTool));
    registry.register(std::sync::Arc::new(WriteTool));
    registry.register(std::sync::Arc::new(ExecCommandTool));

    println!("   Registered tools: {:?}", registry.list_tool_names());
    println!("   Allowed tools: {:?}", registry.list_allowed_tool_names());
    println!();

    // Example 2: Custom restrictive policy
    println!("2. Restrictive Policy Example:");
    let restrictive_policy = ToolPolicy::allow_list(vec!["read_file"]);
    let restricted_registry = ToolRegistry::new_with_policy(restrictive_policy);

    restricted_registry.register(std::sync::Arc::new(ReadFileTool));
    restricted_registry.register(std::sync::Arc::new(WriteTool));
    restricted_registry.register(std::sync::Arc::new(ExecCommandTool));

    println!(
        "   Registered tools: {:?}",
        restricted_registry.list_tool_names()
    );
    println!(
        "   Allowed tools: {:?}",
        restricted_registry.list_allowed_tool_names()
    );
    println!();

    // Example 3: Attempting to execute tools under restrictive policy
    println!("3. Tool Execution Under Restrictive Policy:");

    // This should fail (write is not in allow list)
    let write_result = restricted_registry
        .execute(
            "write",
            json!({
                "path": "/tmp/test.txt",
                "content": "Hello, World!"
            }),
        )
        .await;
    println!(
        "   Write tool execution: {}",
        match write_result {
            Ok(_) => "SUCCESS".to_string(),
            Err(e) => format!("FAILED: {} (as expected)", e),
        }
    );
    println!();

    // Example 4: Blacklist policy
    println!("4. Blacklist (Deny List) Policy Example:");
    let deny_policy = ToolPolicy::deny_list(vec!["exec_command"]);
    let deny_registry = ToolRegistry::new_with_policy(deny_policy);

    deny_registry.register(std::sync::Arc::new(ReadFileTool));
    deny_registry.register(std::sync::Arc::new(WriteTool));
    deny_registry.register(std::sync::Arc::new(ExecCommandTool));

    println!("   Registered tools: {:?}", deny_registry.list_tool_names());
    println!(
        "   Allowed tools: {:?}",
        deny_registry.list_allowed_tool_names()
    );

    let exec_result = deny_registry
        .execute("exec_command", json!({"cmd": "echo hello"}))
        .await;
    println!(
        "   exec_command tool execution: {}",
        match exec_result {
            Ok(_) => "SUCCESS".to_string(),
            Err(e) => format!("FAILED: {} (as expected)", e),
        }
    );
    println!();

    // Example 5: Dynamic policy updates
    println!("5. Dynamic Policy Updates:");
    let mutable_policy = ToolPolicy::allow_list(vec!["read_file"]);
    let mut mutable_registry = ToolRegistry::new_with_policy(mutable_policy);

    println!(
        "   Initial allowed tools: {:?}",
        mutable_registry.list_allowed_tool_names()
    );

    let new_policy = ToolPolicy::allow_list(vec!["read_file", "write"]);
    mutable_registry.update_policy(new_policy);

    println!(
        "   After policy update, allowed tools: {:?}",
        mutable_registry.list_allowed_tool_names()
    );
    println!();

    // Example 6: Policy inspection
    println!("6. Policy Inspection:");
    println!(
        "   Current policy type: {:?}",
        mutable_registry.get_policy().policy_type
    );
    println!("   Policy tools: {:?}", mutable_registry.get_policy().tools);
    println!(
        "   Policy enabled: {}",
        mutable_registry.get_policy().enabled
    );
    println!();

    println!("=== Enhanced Tool System Demo Complete ===");

    Ok(())
}
