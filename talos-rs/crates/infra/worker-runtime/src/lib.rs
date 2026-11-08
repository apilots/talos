//! Host resource management for Workers whose lifecycle is owned by Talos.
//!
//! The caller serializes mutations for each instance and persists desired state.
//! Host observations are independent of Worker registration and RPC readiness.

mod docker;

pub use docker::{DockerEngine, DEFAULT_SECCOMP_PROFILE};

use std::{future::Future, path::PathBuf, pin::Pin};

use uuid::Uuid;

/// A stable resource identity, reused across resets and retries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeIdentity {
    pub instance_id: Uuid,
    /// Eight lowercase ASCII letters or digits, allocated by the caller.
    pub short_id: String,
}

/// Desired compute revision and deployment-controlled image and resource limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerSpec {
    pub identity: RuntimeIdentity,
    pub revision: u64,
    pub image: String,
    pub cpu_cores: u32,
    pub memory_bytes: u64,
}

/// Storage capacity to provision and enforce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeCapacity {
    Unrestricted,
    LimitedBytes(u64),
}

/// Deployment-supplied configuration, written as a private file under Home.
pub struct SeedFile {
    path: PathBuf,
    contents: Vec<u8>,
}

impl SeedFile {
    pub fn new(path: impl Into<PathBuf>, contents: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            contents,
        }
    }
}

/// Existing files are preserved during provisioning and replaced only on request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedMode {
    CreateMissing,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesiredRuntimeState {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Missing,
    Stopped,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeObservation {
    pub container: ContainerState,
    pub home_exists: bool,
}

/// A host failure. Callers translate this to their domain error without exposing sources.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Invalid Worker runtime input: {0}")]
    InvalidSpec(&'static str),
    #[error("This Worker runtime does not enforce a Home capacity limit")]
    UnsupportedCapacity,
    #[error("Worker runtime resource is missing: {resource}")]
    Missing { resource: &'static str },
    #[error("Worker runtime resource conflicts with the requested identity or configuration: {resource}")]
    Conflict { resource: &'static str },
    #[error("Worker runtime resource did not reach the expected state: {resource}")]
    UnexpectedState { resource: &'static str },
    #[error("Could not {operation} through Docker Engine")]
    Docker {
        operation: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("Could not supply a Worker Home file to Docker Engine")]
    SeedIo(#[source] std::io::Error),
}

pub type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + Send + 'a>>;

/// The Server supplies trusted configuration and verifies ownership and Worker readiness.
/// The runtime verifies infrastructure identity before mutating named host resources.
pub trait ManagedWorkerRuntime: Send + Sync {
    /// Create or reuse a Home. Files may only be supplied while the Worker is stopped.
    fn ensure_home<'a>(
        &'a self,
        spec: &'a WorkerSpec,
        capacity: HomeCapacity,
        files: &'a [SeedFile],
        mode: SeedMode,
    ) -> RuntimeFuture<'a, ()>;

    /// Create stopped compute for this revision and attach the existing Home.
    fn ensure_runtime<'a>(&'a self, spec: &'a WorkerSpec) -> RuntimeFuture<'a, ()>;

    /// Start or stop the exact compute revision.
    fn set_running<'a>(
        &'a self,
        spec: &'a WorkerSpec,
        desired: DesiredRuntimeState,
    ) -> RuntimeFuture<'a, ()>;

    fn inspect<'a>(&'a self, spec: &'a WorkerSpec) -> RuntimeFuture<'a, RuntimeObservation>;

    /// Remove stopped compute of the specified revision and retain Home.
    fn remove_runtime<'a>(&'a self, spec: &'a WorkerSpec) -> RuntimeFuture<'a, ()>;

    /// Delete Home once no Worker container remains.
    fn remove_home<'a>(&'a self, identity: &'a RuntimeIdentity) -> RuntimeFuture<'a, ()>;
}
