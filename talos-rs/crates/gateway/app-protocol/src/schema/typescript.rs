use ts_rs::TS;

pub(super) fn export<T: TS>() -> String {
    format!("export {}\n\n", T::decl())
}

pub(super) fn export_worker_runtime(output: &mut String) {
    output.push_str(&export::<talos_types::WorkerRuntimeCandidate>());
    output.push_str(&export::<talos_types::WorkerRuntimeSettingsResponse>());
    output.push_str(&export::<talos_types::WorkerRuntimeBackend>());
    output.push_str(&export::<talos_types::WorkerKubernetesSettings>());
    output.push_str(&export::<talos_types::WorkerKubeconfigAuthentication>());
    output.push_str(&export::<talos_types::WorkerKubeconfigSource>());
}
