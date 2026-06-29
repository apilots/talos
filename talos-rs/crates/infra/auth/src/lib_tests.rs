use super::*;

#[test]
fn auth_disabled_identity_uses_anonymous_user_snapshot() {
    let identity = RequestIdentity::auth_disabled();

    assert_eq!(identity.user_id, "anonymous");
    assert_eq!(identity.username.as_deref(), Some("anonymous"));
    assert!(identity.auth_disabled);
}
