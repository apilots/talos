use super::*;

#[test]
fn app_server_error_body_is_protocol_only() {
    assert_eq!(
        AppServerErrorBody::parse_error("bad json").code,
        PARSE_ERROR
    );
    assert_eq!(
        AppServerErrorBody::method_not_found("session/read").code,
        METHOD_NOT_FOUND
    );
    assert_eq!(AppServerErrorBody::not_initialized().code, NOT_INITIALIZED);
}

#[test]
fn protocol_error_code_rejects_unregistered_numeric_values() {
    let error =
        serde_json::from_str::<AppServerErrorBody>(r#"{"code":-32000,"message":"unregistered"}"#)
            .expect_err("unregistered protocol code should fail");

    assert!(error.is_data());
}
