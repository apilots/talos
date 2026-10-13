#[tokio::test]
async fn current_worker_report_is_typed_and_contains_real_execution_environment_information() {
    let info = super::collect().await.unwrap();
    let expected = if cfg!(target_os = "macos") {
        talos_types::NodeOperatingSystem::MacOs
    } else {
        talos_types::NodeOperatingSystem::Linux
    };
    assert_eq!(info.os, expected);
    assert_eq!(info.architecture, std::env::consts::ARCH);
    assert!(info.logical_cpu_count > 0);
    assert!(info.memory_bytes > 0);
    if cfg!(target_os = "macos") {
        assert!(info.cpu_model.is_some());
    }
    assert!(info.network_interfaces.len() <= 32);
    assert_eq!(info.worker_version, env!("CARGO_PKG_VERSION"));
}
