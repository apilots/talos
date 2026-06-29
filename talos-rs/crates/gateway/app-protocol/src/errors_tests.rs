use super::*;

#[test]
fn maps_common_not_found_error() {
    let body = AppServerErrorBody::from_talos_error(TalosError::ResourceNotFound(
        "session s1".to_string(),
    ));
    assert_eq!(body.code, NOT_FOUND);
    assert_eq!(body.message, "resource not found: session s1");
}
