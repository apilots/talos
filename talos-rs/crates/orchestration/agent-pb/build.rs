fn main() -> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "auth.proto",
        "desktop.proto",
        "coordinator.proto",
        "memory.proto",
        "node.proto",
        "connection.proto",
        "environment.proto",
        "environment_browser.proto",
        "environment_skills.proto",
        "environment_memory.proto",
        "environment_config.proto",
        "environment_plugins.proto",
        "harness.proto",
        "resources.proto",
        "types.proto",
    ] {
        println!("cargo:rerun-if-changed=proto/{source}");
    }
    tonic_prost_build::configure()
        // agent-client owns channels; the generated Connect RPC owns this method name.
        .build_transport(false)
        // Box only the new carrier oneofs; existing business messages and wire fields stay unchanged.
        .boxed(".talos.agent.v1.WorkerRpcCall.request")
        .boxed(".talos.agent.v1.WorkerRpcResult.response")
        .compile_protos(
            &[
                "proto/auth.proto",
                "proto/desktop.proto",
                "proto/coordinator.proto",
                "proto/memory.proto",
                "proto/node.proto",
                "proto/connection.proto",
                "proto/environment.proto",
                "proto/environment_browser.proto",
                "proto/environment_skills.proto",
                "proto/environment_memory.proto",
                "proto/environment_config.proto",
                "proto/environment_plugins.proto",
                "proto/harness.proto",
                "proto/resources.proto",
                "proto/types.proto",
            ],
            &["proto"],
        )?;
    Ok(())
}
