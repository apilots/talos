fn main() -> Result<(), Box<dyn std::error::Error>> {
    for source in [
        "auth.proto",
        "automation.proto",
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
        // Keep the typed automation receipt from inflating its result enum.
        .boxed(".talos.agent.v1.ExecuteAutomationResponse.result")
        // Keep protocol oneof carriers small without changing their wire representation.
        .boxed(".talos.agent.v1.CompactSessionParams.body")
        .boxed(".talos.agent.v1.ConfigRead.value.llm")
        .boxed(".talos.agent.v1.ConfigRead.value.web_search")
        .boxed(".talos.agent.v1.ConfigUpdateParams.value.llm")
        .boxed(".talos.agent.v1.ConfigUpdateParams.value.web_search")
        .boxed(".talos.agent.v1.CreateNodeResponse.result")
        .boxed(".talos.agent.v1.DisablePluginResponse.result")
        .boxed(".talos.agent.v1.EnablePluginResponse.result")
        .boxed(".talos.agent.v1.GetConfigResponse.result")
        .boxed(".talos.agent.v1.GetNodeResponse.result")
        .boxed(".talos.agent.v1.ImportMcpServerResponse.result")
        .boxed(".talos.agent.v1.InstallPluginResponse.result")
        .boxed(".talos.agent.v1.PluginDefinition.value")
        .boxed(".talos.agent.v1.PluginInstallTarget.value")
        .boxed(".talos.agent.v1.PluginPrepareParams.change")
        .boxed(".talos.agent.v1.ReadPluginResponse.result")
        .boxed(".talos.agent.v1.RunResponse.result")
        .boxed(".talos.agent.v1.StartParams.body")
        .boxed(".talos.agent.v1.UpdateConfigResponse.result")
        .boxed(".talos.agent.v1.UpdatePluginResponse.result")
        .compile_protos(
            &[
                "proto/auth.proto",
                "proto/automation.proto",
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
