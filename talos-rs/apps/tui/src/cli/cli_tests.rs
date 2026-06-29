use super::*;

#[test]
fn parses_exec_command() {
    let cli = Cli::try_parse_from(["talos", "exec", "hello"]);
    assert!(cli.is_ok());
}

#[test]
fn parses_tui_default_command() {
    let cli = Cli::try_parse_from(["talos"]);
    assert!(cli.is_ok());
}
