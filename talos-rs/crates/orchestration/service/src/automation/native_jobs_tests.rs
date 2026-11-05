use super::*;
use crate::native_jobs::{NativeJobFuture, NativeJobRuntime};
use std::sync::Mutex as StateMutex;
use talos_store::automation::native_jobs::NativeJobStoreFuture;
use talos_types::BuiltinJobId;

type Log = Arc<StateMutex<Vec<&'static str>>>;
struct Controls {
    log: Log,
    paused: StateMutex<bool>,
    fail: bool,
}
impl NativeJobControlStore for Controls {
    fn load_paused<'a>(&'a self, _: &'a str) -> NativeJobStoreFuture<'a, Vec<BuiltinJobId>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn set_paused<'a>(&'a self, update: SetNativeJobPaused<'a>) -> NativeJobStoreFuture<'a, ()> {
        Box::pin(async move {
            self.log.lock().unwrap().push("persist");
            tokio::task::yield_now().await;
            if self.fail {
                return Err(TalosError::internal("Expected persistence failure."));
            }
            *self.paused.lock().unwrap() = update.paused;
            Ok(())
        })
    }
}
struct Runtime {
    log: Log,
    info: StateMutex<BuiltinJobInfo>,
    fail_apply: bool,
}
impl NativeJobRuntime for Runtime {
    fn list(&self) -> NativeJobFuture<'_, Vec<BuiltinJobInfo>> {
        Box::pin(async { Ok(vec![self.info.lock().unwrap().clone()]) })
    }
    fn set_paused(&self, update: NativeJobPauseUpdate) -> NativeJobFuture<'_, ()> {
        Box::pin(async move {
            self.log.lock().unwrap().push("apply");
            if self.fail_apply {
                return Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Expected runtime failure.",
                ));
            }
            self.info.lock().unwrap().paused = update.paused;
            Ok(())
        })
    }
    fn run_once(&self, _: BuiltinJobId) -> NativeJobFuture<'_, ()> {
        Box::pin(async {
            let mut info = self.info.lock().unwrap();
            if info.running {
                return Err(TalosError::new(ErrorCode::Conflict, "Already running."));
            }
            info.running = true;
            self.log.lock().unwrap().push("run");
            Ok(())
        })
    }
}
struct Host {
    runtime: Arc<Runtime>,
}
impl NativeJobHost for Host {
    fn resolve_owned<'a>(
        &'a self,
        actor: &'a Actor,
        computer: &'a str,
    ) -> NativeJobFuture<'a, ResolvedNativeJobs> {
        Box::pin(async move {
            if actor.user_id != "owner" || computer != "owned-computer" {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "This account does not own the native job host.",
                ));
            }
            Ok(ResolvedNativeJobs {
                computer_id: "owned-computer".into(),
                execution_instance_id: "actual-instance".into(),
                runtime: self.runtime.clone(),
            })
        })
    }
}
fn info() -> BuiltinJobInfo {
    BuiltinJobInfo {
        id: BuiltinJobId::ProfileUsageRetention,
        computer_id: "owned-computer".into(),
        execution_instance_id: "actual-instance".into(),
        name: "Profile retention".into(),
        schedule: "0 15 3 * * 1".into(),
        timezone: "Asia/Shanghai".into(),
        paused: false,
        running: false,
        next_due_at: None,
        last_finished_at: None,
        last_outcome: None,
        available_actions: vec![
            BuiltinJobAction::Pause,
            BuiltinJobAction::Resume,
            BuiltinJobAction::RunOnce,
        ],
    }
}
fn fixture(
    fail_store: bool,
    fail_apply: bool,
) -> (
    NativeJobService,
    Arc<dyn NativeJobHost>,
    Arc<Controls>,
    Arc<Runtime>,
    Log,
) {
    let log = Arc::new(StateMutex::new(vec![]));
    let controls = Arc::new(Controls {
        log: log.clone(),
        paused: StateMutex::new(false),
        fail: fail_store,
    });
    let runtime = Arc::new(Runtime {
        log: log.clone(),
        info: StateMutex::new(info()),
        fail_apply,
    });
    let host: Arc<dyn NativeJobHost> = Arc::new(Host {
        runtime: runtime.clone(),
    });
    (
        NativeJobService::new(controls.clone(), Arc::downgrade(&host)),
        host,
        controls,
        runtime,
        log,
    )
}
fn request(action: BuiltinJobAction) -> BuiltinJobControlRequest {
    BuiltinJobControlRequest {
        computer_id: "owned-computer".into(),
        execution_instance_id: "actual-instance".into(),
        job_id: BuiltinJobId::ProfileUsageRetention,
        action,
    }
}

#[tokio::test]
async fn native_job_owner_and_instance_checks_precede_persistence() {
    let (service, _host, _, _, log) = fixture(false, false);
    assert!(service
        .control(&Actor::regular("other"), request(BuiltinJobAction::Pause))
        .await
        .unwrap_err()
        .is_code(ErrorCode::PermissionDenied));
    let mut stale = request(BuiltinJobAction::Pause);
    stale.execution_instance_id = "other-instance".into();
    assert!(service
        .control(&Actor::regular("owner"), stale)
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert!(log.lock().unwrap().is_empty());
}
#[tokio::test]
async fn native_job_control_persists_before_apply_and_manual_run_keeps_pause() {
    let (service, _host, controls, runtime, log) = fixture(false, false);
    let actor = Actor::regular("owner");
    assert!(
        service
            .control(&actor, request(BuiltinJobAction::Pause))
            .await
            .unwrap()
            .paused
    );
    assert_eq!(*log.lock().unwrap(), vec!["persist", "apply"]);
    assert!(*controls.paused.lock().unwrap());
    let running = service
        .control(&actor, request(BuiltinJobAction::RunOnce))
        .await
        .unwrap();
    assert!(running.paused && running.running);
    assert!(service
        .control(&actor, request(BuiltinJobAction::RunOnce))
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert!(runtime.info.lock().unwrap().last_outcome.is_none());
}
#[tokio::test]
async fn native_job_storage_failure_never_changes_runtime() {
    let (service, _host, _, runtime, log) = fixture(true, false);
    assert!(service
        .control(&Actor::regular("owner"), request(BuiltinJobAction::Pause))
        .await
        .is_err());
    assert_eq!(*log.lock().unwrap(), vec!["persist"]);
    assert!(!runtime.info.lock().unwrap().paused);
}
#[tokio::test]
async fn native_job_apply_failure_remains_explicit_and_durable() {
    let (service, _host, controls, runtime, _) = fixture(false, true);
    assert!(service
        .control(&Actor::regular("owner"), request(BuiltinJobAction::Pause))
        .await
        .unwrap_err()
        .is_code(ErrorCode::ServiceUnavailable));
    assert!(*controls.paused.lock().unwrap());
    assert!(!runtime.info.lock().unwrap().paused);
}
#[tokio::test]
async fn native_job_host_is_weak_and_mismatched_runtime_scope_is_rejected() {
    let (service, host, _, runtime, _) = fixture(false, false);
    let actor = Actor::regular("owner");
    runtime.info.lock().unwrap().computer_id = "other-computer".into();
    assert!(service
        .list(
            &actor,
            BuiltinJobListRequest {
                computer_id: "owned-computer".into()
            }
        )
        .await
        .is_err());
    drop(host);
    assert!(service
        .list(
            &actor,
            BuiltinJobListRequest {
                computer_id: "owned-computer".into()
            }
        )
        .await
        .unwrap_err()
        .is_code(ErrorCode::ServiceUnavailable));
}
#[tokio::test]
async fn native_job_unadvertised_action_and_unregistered_job_do_not_write() {
    let (service, _host, _, runtime, log) = fixture(false, false);
    runtime.info.lock().unwrap().available_actions.clear();
    assert!(service
        .control(&Actor::regular("owner"), request(BuiltinJobAction::RunOnce))
        .await
        .unwrap_err()
        .is_code(ErrorCode::InvalidRequest));
    let mut missing = request(BuiltinJobAction::Pause);
    missing.job_id = BuiltinJobId::DreamMemoryMaintenance;
    assert!(service
        .control(&Actor::regular("owner"), missing)
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    assert!(log.lock().unwrap().is_empty());
}

#[tokio::test]
async fn native_job_concurrent_pause_resume_preserves_persistence_application_order() {
    let (service, _host, controls, runtime, log) = fixture(false, false);
    let actor = Actor::regular("owner");
    let (paused, resumed) = tokio::join!(biased;
        service.control(&actor, request(BuiltinJobAction::Pause)),
        service.control(&actor, request(BuiltinJobAction::Resume)),
    );
    assert!(paused.unwrap().paused);
    assert!(!resumed.unwrap().paused);
    assert_eq!(
        *log.lock().unwrap(),
        vec!["persist", "apply", "persist", "apply"]
    );
    assert!(!*controls.paused.lock().unwrap());
    assert!(!runtime.info.lock().unwrap().paused);
}
