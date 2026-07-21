use serde::{Deserialize, Serialize};

/// Configuration for application-owned scheduled maintenance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct AutomationConfig {
    #[serde(default)]
    pub profile_usage_retention: ProfileUsageRetentionConfig,
    #[serde(default)]
    pub memory_daily_maintenance: MemoryDailyMaintenanceConfig,
}

/// Bounded global schedule for daily memory formation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryDailyMaintenanceConfig {
    #[serde(default = "default_memory_daily_maintenance_cron")]
    pub cron: String,
    #[serde(default = "default_memory_daily_maintenance_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_memory_daily_maintenance_max_batches")]
    pub max_batches: usize,
}

impl Default for MemoryDailyMaintenanceConfig {
    fn default() -> Self {
        Self {
            cron: default_memory_daily_maintenance_cron(),
            batch_size: default_memory_daily_maintenance_batch_size(),
            max_batches: default_memory_daily_maintenance_max_batches(),
        }
    }
}

/// Retention policy for finalized rows in `profile_turn_usage`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfileUsageRetentionConfig {
    #[serde(default = "default_profile_usage_retention_cron")]
    pub cron: String,

    #[serde(default = "default_profile_usage_retention_days")]
    pub retention_days: u32,

    #[serde(default = "default_profile_usage_retention_batch_size")]
    pub batch_size: usize,

    #[serde(default = "default_profile_usage_retention_max_batches")]
    pub max_batches: usize,
}

impl Default for ProfileUsageRetentionConfig {
    fn default() -> Self {
        Self {
            cron: default_profile_usage_retention_cron(),
            retention_days: default_profile_usage_retention_days(),
            batch_size: default_profile_usage_retention_batch_size(),
            max_batches: default_profile_usage_retention_max_batches(),
        }
    }
}

fn default_profile_usage_retention_cron() -> String {
    "0 15 3 * * 1".to_string()
}

fn default_memory_daily_maintenance_cron() -> String {
    "0 30 3 * * *".to_string()
}

fn default_memory_daily_maintenance_batch_size() -> usize {
    50
}

fn default_memory_daily_maintenance_max_batches() -> usize {
    10
}

fn default_profile_usage_retention_days() -> u32 {
    366
}

fn default_profile_usage_retention_batch_size() -> usize {
    1_000
}

fn default_profile_usage_retention_max_batches() -> usize {
    10
}
