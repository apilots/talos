use super::*;

#[test]
fn upgrade_before_skips_default_config_when_explicit_config_is_set() {
    let report = upgrade_before_with_explicit_config(/*explicit_config*/ true)
        .expect("upgrade should skip default config");

    assert_eq!(
        report,
        BeforeUpgradeReport {
            default_config_template: None,
            default_config: None,
            skipped_explicit_config: true,
        }
    );
}
