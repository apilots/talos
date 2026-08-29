use super::*;

#[test]
fn explicit_config_skips_default_config_preparation() {
    let report = prepare_before_config_load_with_explicit_config(/*explicit_config*/ true)
        .expect("preparation should skip default config");

    assert_eq!(
        report,
        BeforeConfigLoadReport {
            default_config_template: None,
            default_config: None,
            skipped_explicit_config: true,
        }
    );
}
