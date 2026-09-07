use std::error::Error;

use talos_agent::AgentRunFailure;
use talos_error::{ErrorCode, TalosError};

#[tokio::test]
async fn product_codes_survive_agent_task_propagation() {
    for code in [
        ErrorCode::PermissionDenied,
        ErrorCode::Unauthenticated,
        ErrorCode::SessionBudgetExceeded,
        ErrorCode::UsageLimitExceeded,
        ErrorCode::UnsupportedAttachment,
        ErrorCode::PathNotFound,
        ErrorCode::ProviderUnavailable,
    ] {
        let original = TalosError::new(code, "specific diagnostic");
        let expected = original.to_api_error();
        let result = tokio::spawn(async move { original }).await.unwrap();
        assert_eq!(result.to_api_error(), expected);
    }
}

#[tokio::test]
async fn source_and_capture_location_survive_agent_task_propagation() {
    let original = TalosError::internal_with_source(
        "read session",
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "storage unavailable"),
    );
    let location = original.location();
    let propagated = original.clone();
    let result = tokio::spawn(async move { propagated.context("resume turn") })
        .await
        .unwrap();
    let source = result
        .source()
        .and_then(|source| source.downcast_ref::<std::io::Error>());
    assert_eq!(
        source.map(std::io::Error::kind),
        Some(std::io::ErrorKind::PermissionDenied)
    );
    assert_eq!(result.location(), location);
    assert!(std::ptr::eq(result.backtrace(), original.backtrace()));
}

#[test]
fn terminal_retry_policy_preserves_the_product_error() {
    let original = TalosError::new(ErrorCode::ProviderUnavailable, "provider unavailable");
    let expected = original.to_api_error();
    let failure = AgentRunFailure::from(original.clone()).retryable();

    assert!(failure.is_retryable());
    assert_eq!(failure.error().to_api_error(), expected);
    assert!(std::ptr::eq(
        failure.error().backtrace(),
        original.backtrace()
    ));
}
