use super::*;
use talos_config::{
    Config, DmPolicy as ConfigDmPolicy, GroupPolicy as ConfigGroupPolicy, TelegramAccountConfig,
    TelegramChannelsConfig, TelegramMode as ConfigTelegramMode,
};

#[test]
fn test_channel_definition() {
    let channel = TelegramChannel::new();
    assert_eq!(channel.definition().id, "telegram");
    assert_eq!(channel.definition().name, "Telegram");
    assert!(channel.definition().capabilities.supports_threads);
    assert!(channel.definition().capabilities.supports_edit);
    assert!(channel.definition().capabilities.supports_typing);
}

#[test]
fn test_parse_dm_policy() {
    assert_eq!(map_dm_policy(ConfigDmPolicy::Open), DmPolicy::Open);
    assert_eq!(map_dm_policy(ConfigDmPolicy::Pairing), DmPolicy::Pairing);
    assert_eq!(
        map_dm_policy(ConfigDmPolicy::Allowlist),
        DmPolicy::Allowlist
    );
}

#[test]
fn test_parse_group_policy() {
    assert_eq!(map_group_policy(ConfigGroupPolicy::Open), GroupPolicy::Open);
    assert_eq!(
        map_group_policy(ConfigGroupPolicy::Mention),
        GroupPolicy::Mention
    );
    assert_eq!(
        map_group_policy(ConfigGroupPolicy::Allowlist),
        GroupPolicy::Allowlist
    );
}

#[test]
fn test_parse_mode() {
    assert_eq!(map_mode(ConfigTelegramMode::Polling), Mode::Polling);
    assert_eq!(map_mode(ConfigTelegramMode::Webhook), Mode::Webhook);
}

#[test]
fn test_enabled_account_ids_reads_multi_account_config() {
    let channel = TelegramChannel::new();
    let mut config = Config::default();
    config.channels.telegram = TelegramChannelsConfig {
        enabled: true,
        token: Some("root-token".to_string()),
        accounts: vec![
            TelegramAccountConfig {
                id: "bot-a".to_string(),
                enabled: true,
                token: Some("token-a".to_string()),
                ..Default::default()
            },
            TelegramAccountConfig {
                id: "bot-b".to_string(),
                enabled: false,
                token: Some("token-b".to_string()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    assert_eq!(
        channel.enabled_account_ids(&config),
        vec!["bot-a".to_string()]
    );
}
