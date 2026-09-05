use super::*;
use std::path::PathBuf;

#[test]
fn json_schema_fixture_is_current() {
    let expected = generate_json_schema();
    let actual = std::fs::read_to_string(schema_fixture_path())
        .expect("schema fixture should be checked in");
    assert_eq!(actual, expected);
}

#[test]
fn typescript_fixture_is_current() {
    let expected = generate_typescript();
    let actual = std::fs::read_to_string(typescript_fixture_path())
        .expect("typescript fixture should be checked in");
    assert_eq!(actual, expected);
}

fn schema_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas/app-protocol.schema.json")
}

fn typescript_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts")
}
