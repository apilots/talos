//! Example demonstrating the TUI interface capabilities
//!
//! Run with: cargo run -p talos-ui-tui --example tui_demo

fn main() {
    println!("Talos TUI Example");
    println!("=================");
    println!("The TUI runs through embedded JSON-RPC stdio.");
    println!();
    println!("To run the TUI:");
    println!("  # With embedded app-server (requires <talos_home>/conf/talos.yaml):");
    println!("  cargo run -p talos-ui-tui --bin talos");
    println!();
    println!("  # Override embedded app-server binary:");
    println!(
        "  cargo run -p talos-ui-tui --bin talos -- --app-server-bin ./target/debug/app-server"
    );
    println!();
    println!("Key features:");
    println!("- Welcome header with model/mode/workspace");
    println!("- Slash command support: /help /model /models /status /quit");
    println!("- Streaming conversation via JSON-RPC events");
    println!("- Input prompt with keyboard shortcuts (Ctrl+C, Enter)");
}
