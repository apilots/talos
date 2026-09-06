use super::*;

#[test]
fn retry_policy_and_context_preserve_the_product_error() {
    let original = TalosError::localized(talos_error::ErrorLocalization::ModelNotConfigured {
        provider: "local".into(),
        model: "missing".into(),
    });
    let expected = original.to_api_error();
    let error = AgentError::from(original.clone());
    assert!(!error.retryable);

    let error = error.retryable(true).context("resume agent run");
    assert!(error.retryable);
    assert_eq!(error.error().to_api_error(), expected);
    assert!(std::ptr::eq(
        error.error().backtrace(),
        original.backtrace()
    ));
}
