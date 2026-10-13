use super::*;
#[test]
fn plaintext_and_credential_urls_are_rejected_even_when_disabled() {
    for endpoint in [
        "ws://localhost/v1/sync",
        "https://localhost/v1/sync",
        "wss://token@localhost/v1/sync",
        "wss://localhost/v1/sync?token=secret",
    ] {
        let config = SyncConfig {
            enabled: Some(false),
            hub_endpoint: endpoint.into(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
    assert!(SyncConfig::default().validate().is_ok());
}
