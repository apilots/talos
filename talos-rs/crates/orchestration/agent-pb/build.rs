fn main() -> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "auth.proto",
        "memory.proto",
        "node.proto",
        "harness.proto",
        "resources.proto",
        "types.proto",
    ] {
        println!("cargo:rerun-if-changed=proto/{source}");
    }
    tonic_prost_build::configure().compile_protos(
        &[
            "proto/auth.proto",
            "proto/memory.proto",
            "proto/node.proto",
            "proto/harness.proto",
            "proto/resources.proto",
            "proto/types.proto",
        ],
        &["proto"],
    )?;
    Ok(())
}
