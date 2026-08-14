use super::build_welcome_card;
use std::path::Path;

#[test]
fn build_welcome_card_separates_title_and_session_block() {
    let card = build_welcome_card(
        "v0.1.5",
        "qwen-plus",
        Some(Path::new("/Users/xujiang/data/ailabs/talos/talos-rs")),
        talos_sdk::UserPermissionMode::FullAccess,
    );

    let lines: Vec<&str> = card.lines().collect();
    assert_eq!(
        lines[1],
        "│ >_ Talos TUI (v0.1.5)                            │"
    );
    assert!(lines[3].starts_with("│ model:"));
    assert!(lines[3].contains("/model to change"));
    assert!(lines[4].starts_with("│ directory:"));
    assert!(lines[5].starts_with("│ permissions:"));
    assert!(lines[5].contains("Full access"));
    assert_eq!(
        lines[6],
        "╰──────────────────────────────────────────────────╯"
    );
}
