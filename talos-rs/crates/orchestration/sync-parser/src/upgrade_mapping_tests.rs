use super::*;
#[test]
fn deployed_upgrade_profile_is_generated_by_the_header_owner() {
    let generated = v1_upgrade_mapping_json().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../conf/sync-v1-upgrade-mapping.json");
    assert_eq!(std::fs::read_to_string(path).unwrap(), generated);
}
