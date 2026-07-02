use super::*;
use std::path::PathBuf;

#[test]
fn schema_generation_is_stable_json() {
    let schema = generate_json_schema();
    assert!(schema.contains("AppProtocolSchemaRoot"));
}

#[test]
fn typescript_generation_includes_jsonrpc_message() {
    let typescript = generate_typescript();
    assert!(typescript.contains("JsonRpcMessage"));
}

#[test]
fn generated_protocol_includes_timeline_types() {
    let schema = generate_json_schema();
    assert!(schema.contains("AgentEvent"));
    assert!(schema.contains("TalosTimelineItem"));

    let typescript = generate_typescript();
    assert!(typescript.contains("AgentEvent"));
    assert!(typescript.contains("TalosTimelineDelta"));
}

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
