//! Example demonstrating the TUI interface capabilities

use std::sync::Arc;
use talos_agent::MockAgent;
use talos_ui_tui::TuiApp;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Talos TUI Example");
    println!("=================");
    println!("This example shows the TUI interface capabilities.");
    println!("In a real implementation, you would interact with the terminal interface.");
    println!();

    // For this example, we'll just show how to create the app
    let mock_agent = Arc::new(MockAgent::new());

    // Create the TUI app
    let app = TuiApp::new_with_agent(mock_agent);

    println!("TUI App created with:");
    println!("- Current state: {:?}", app.get_current_state());
    println!("- Chat history items: {}", app.get_chat_history().len());
    println!("- Input buffer: {:?}", app.get_input_buffer());
    println!("- Tool status items: {}", app.get_tool_status().len());

    println!();
    println!("To run the actual TUI interface, use: cargo run -p talos-ui-tui");
    println!();
    println!("Key features implemented:");
    println!("- Welcome header with model/mode/workspace");
    println!("- Slash command support: /help /model /models /status /quit");
    println!("- Single-column conversation view with collapsible trace panel (Ctrl+T)");
    println!("- Input prompt with custom marker and in-app cursor");
    println!("- Keyboard shortcuts (Ctrl+C to quit, Enter to submit)");

    Ok(())
}
