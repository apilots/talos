use crate::types::{AccessDecision, DmPolicy, GroupPolicy};

#[derive(Clone)]
pub struct PolicyChecker {
    pub dm_policy: DmPolicy,
    pub group_policy: GroupPolicy,
    pub dm_allow_from: Vec<String>,
    pub group_allow_from: Vec<String>,
}

impl PolicyChecker {
    pub fn new(
        dm_policy: DmPolicy,
        group_policy: GroupPolicy,
        dm_allow_from: Vec<String>,
        group_allow_from: Vec<String>,
    ) -> Self {
        Self {
            dm_policy,
            group_policy,
            dm_allow_from,
            group_allow_from,
        }
    }

    pub fn check_dm_access(&self, user_id: &str, is_paired: bool) -> AccessDecision {
        match self.dm_policy {
            DmPolicy::Open => AccessDecision::Allowed,
            DmPolicy::Pairing => {
                if is_paired || self.dm_allow_from.contains(&user_id.to_string()) {
                    AccessDecision::Allowed
                } else {
                    AccessDecision::NeedPairing
                }
            }
            DmPolicy::Allowlist => {
                if self.dm_allow_from.contains(&user_id.to_string()) {
                    AccessDecision::Allowed
                } else {
                    AccessDecision::Denied
                }
            }
        }
    }

    pub fn check_group_access(&self, group_id: &str, is_mentioned: bool) -> AccessDecision {
        match self.group_policy {
            GroupPolicy::Open => AccessDecision::Allowed,
            GroupPolicy::Mention => {
                if is_mentioned {
                    AccessDecision::Allowed
                } else {
                    AccessDecision::Denied
                }
            }
            GroupPolicy::Allowlist => {
                if self.group_allow_from.contains(&group_id.to_string()) {
                    AccessDecision::Allowed
                } else {
                    AccessDecision::Denied
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
