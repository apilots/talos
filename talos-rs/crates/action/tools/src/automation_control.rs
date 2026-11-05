use std::future::Future;
use std::pin::Pin;

use talos_error::TalosResult;
use talos_types::{AutomationControlResult, AutomationModelRequest};

/// Object-safe future for authenticated automation operations.
pub type AutomationControlFuture<'a> =
    Pin<Box<dyn Future<Output = TalosResult<AutomationControlResult>> + Send + 'a>>;

/// Restricted Tool-facing port implemented by existing application composition.
///
/// Callers come from invocation context. The owning Service validates the real
/// Session/Turn, explicit activation intent and object/revision binding. This
/// port neither schedules work nor grants access to native maintenance jobs.
pub trait AutomationControl: Send + Sync {
    fn execute(&self, request: AutomationModelRequest) -> AutomationControlFuture<'_>;
}
