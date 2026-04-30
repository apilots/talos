use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let schema_path = manifest_dir.join("schemas/app-protocol.schema.json");
    let ts_path = manifest_dir
        .join("../../../../talos-app/ts-sdk/packages/core/src/generated/app-protocol.ts");

    fs::create_dir_all(schema_path.parent().expect("schema parent should exist"))
        .expect("schema directory should be created");
    fs::create_dir_all(ts_path.parent().expect("typescript parent should exist"))
        .expect("typescript directory should be created");

    fs::write(
        schema_path,
        talos_app_protocol::schema::generate_json_schema(),
    )
    .expect("schema fixture should be written");
    fs::write(ts_path, talos_app_protocol::schema::generate_typescript())
        .expect("typescript fixture should be written");
}
