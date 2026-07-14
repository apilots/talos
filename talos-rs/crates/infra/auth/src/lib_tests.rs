use super::*;

#[test]
fn auth_disabled_identity_uses_anonymous_user_snapshot() {
    let identity = RequestIdentity::auth_disabled();

    assert_eq!(identity.user_id, "anonymous");
    assert_eq!(identity.username.as_deref(), Some("anonymous"));
    assert!(identity.auth_disabled);
}

#[test]
fn user_type_uses_snake_case_wire_values() {
    assert_eq!(serde_json::to_string(&UserType::Max).unwrap(), r#""max""#);
}
