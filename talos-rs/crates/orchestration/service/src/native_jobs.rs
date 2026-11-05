//! Ports into the one existing application-owned maintenance scheduler.
//!
//! Application composition authenticates the real computer/execution instance.
//! Service holds the host weakly: native Dream callbacks can themselves retain
//! the Service, so a strong Service-to-scheduler link would form a cycle.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use talos_error::TalosResult;
use talos_types::{BuiltinJobId, BuiltinJobInfo};

use crate::Actor;

pub type NativeJobFuture<'a, T> = Pin<Box<dyn Future<Output = TalosResult<T>> + Send + 'a>>;

pub struct NativeJobPauseUpdate {
    pub job_id: BuiltinJobId,
    pub paused: bool,
}

/// A scheduler adapter already bound to one actual, authorized host scope.
pub trait NativeJobRuntime: Send + Sync {
    fn list(&self) -> NativeJobFuture<'_, Vec<BuiltinJobInfo>>;
    fn set_paused(&self, update: NativeJobPauseUpdate) -> NativeJobFuture<'_, ()>;
    fn run_once(&self, job_id: BuiltinJobId) -> NativeJobFuture<'_, ()>;
}

pub struct ResolvedNativeJobs {
    pub computer_id: String,
    pub execution_instance_id: String,
    pub runtime: Arc<dyn NativeJobRuntime>,
}

/// Existing host ownership checks stay in app composition, never imported by
/// orchestration Service. An arbitrary worker computer does not own Server jobs.
pub trait NativeJobHost: Send + Sync {
    fn resolve_owned<'a>(
        &'a self,
        actor: &'a Actor,
        computer_id: &'a str,
    ) -> NativeJobFuture<'a, ResolvedNativeJobs>;
}
