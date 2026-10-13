//! Static metadata collection for registration and local Computer presentation.
//! Call once when a Worker starts; this library owns platform differences and
//! collects only information visible to that process. Missing optional hardware
//! is normal in containers. It never supplies identity, authorization or health.

use talos_error::{TalosError, TalosResult};
use talos_types::{NodeNetworkInterface, NodeOperatingSystem, NodeSystemInfo};

/// Collect a bounded static report, automatically selecting Linux or macOS.
/// CPU/RAM/network collection runs off the async executor. GPU inspection is
/// optional and bounded; it cannot prevent a Worker from starting.
pub async fn collect() -> TalosResult<NodeSystemInfo> {
    let mut info = tokio::task::spawn_blocking(collect_system)
        .await
        .map_err(|error| {
            TalosError::internal_with_source("Collect Worker system information.", error)
        })??;
    let backends = match info.os {
        NodeOperatingSystem::Linux => wgpu::Backends::VULKAN,
        NodeOperatingSystem::MacOs => wgpu::Backends::METAL,
    };
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        flags: wgpu::InstanceFlags::empty(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    if let Ok(adapters) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        instance.enumerate_adapters(backends),
    )
    .await
    {
        info.graphics = adapters
            .into_iter()
            .filter_map(|adapter| clean(Some(adapter.get_info().name)))
            .take(16)
            .collect();
        info.graphics.sort();
        info.graphics.dedup();
    }
    Ok(info)
}

fn collect_system() -> TalosResult<NodeSystemInfo> {
    let os = match std::env::consts::OS {
        "linux" => NodeOperatingSystem::Linux,
        "macos" => NodeOperatingSystem::MacOs,
        _ => {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Worker system metadata supports Linux and macOS.",
            ))
        }
    };
    let system = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing()
            .with_cpu(sysinfo::CpuRefreshKind::nothing())
            .with_memory(sysinfo::MemoryRefreshKind::nothing().with_ram()),
    );
    let mut network_interfaces = sysinfo::Networks::new_with_refreshed_list()
        .iter()
        .filter(|(_, data)| !data.mac_address().is_unspecified())
        .map(|(name, data)| NodeNetworkInterface {
            name: name.clone(),
            mac_address: data.mac_address().to_string(),
        })
        .collect::<Vec<_>>();
    network_interfaces.sort_by(|a, b| a.name.cmp(&b.name));
    network_interfaces.truncate(32);
    let board_model = sysinfo::Motherboard::new().and_then(|board| clean(board.name()));
    Ok(NodeSystemInfo {
        os,
        os_version: clean(sysinfo::System::long_os_version()),
        architecture: std::env::consts::ARCH.into(),
        hostname: clean(sysinfo::System::host_name()),
        cpu_model: clean(system.cpus().first().map(|cpu| cpu.brand().into())),
        logical_cpu_count: system
            .cpus()
            .len()
            .try_into()
            .map_err(|error| TalosError::internal_with_source("Count Worker CPUs.", error))?,
        // A container's effective memory ceiling is more useful than its host's total.
        memory_bytes: system
            .cgroup_limits()
            .map_or(system.total_memory(), |limit| {
                limit.total_memory.min(system.total_memory())
            }),
        board_model,
        network_interfaces,
        graphics: Vec::new(),
        worker_version: env!("CARGO_PKG_VERSION").into(),
    })
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().chars().take(255).collect::<String>())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "system_info_tests.rs"]
mod tests;
