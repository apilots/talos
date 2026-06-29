use super::*;

#[test]
fn test_dm_policy_open() {
    let checker = PolicyChecker::new(DmPolicy::Open, GroupPolicy::Open, vec![], vec![]);
    assert_eq!(
        checker.check_dm_access("user1", false),
        AccessDecision::Allowed
    );
}

#[test]
fn test_dm_policy_pairing() {
    let checker = PolicyChecker::new(DmPolicy::Pairing, GroupPolicy::Open, vec![], vec![]);
    assert_eq!(
        checker.check_dm_access("user1", false),
        AccessDecision::NeedPairing
    );
    assert_eq!(
        checker.check_dm_access("user1", true),
        AccessDecision::Allowed
    );
}

#[test]
fn test_dm_policy_allowlist() {
    let checker = PolicyChecker::new(
        DmPolicy::Allowlist,
        GroupPolicy::Open,
        vec!["user1".to_string()],
        vec![],
    );
    assert_eq!(
        checker.check_dm_access("user1", false),
        AccessDecision::Allowed
    );
    assert_eq!(
        checker.check_dm_access("user2", false),
        AccessDecision::Denied
    );
}

#[test]
fn test_group_policy_mention() {
    let checker = PolicyChecker::new(DmPolicy::Open, GroupPolicy::Mention, vec![], vec![]);
    assert_eq!(
        checker.check_group_access("group1", true),
        AccessDecision::Allowed
    );
    assert_eq!(
        checker.check_group_access("group1", false),
        AccessDecision::Denied
    );
}

#[test]
fn test_group_policy_allowlist() {
    let checker = PolicyChecker::new(
        DmPolicy::Open,
        GroupPolicy::Allowlist,
        vec![],
        vec!["group1".to_string()],
    );
    assert_eq!(
        checker.check_group_access("group1", false),
        AccessDecision::Allowed
    );
    assert_eq!(
        checker.check_group_access("group2", false),
        AccessDecision::Denied
    );
}
