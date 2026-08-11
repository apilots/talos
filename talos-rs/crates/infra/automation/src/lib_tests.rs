use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{TimeZone, Utc};

use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cron_job_runs_on_start_and_shutdown_is_idempotent() {
    let calls = Arc::new(AtomicUsize::new(0));
    let task_calls = calls.clone();
    let automation = Automation::start(vec![AutomationJob::cron_on_start(
        "test",
        "0 0 0 * * *",
        chrono_tz::UTC,
        move || {
            let task_calls = task_calls.clone();
            async move {
                task_calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
    )])
    .await
    .expect("scheduler should start");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    automation
        .shutdown()
        .await
        .expect("shutdown should succeed");
    automation
        .shutdown()
        .await
        .expect("shutdown should be idempotent");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn job_failure_is_isolated_from_scheduler_startup() {
    let automation = Automation::start(vec![AutomationJob::cron_on_start(
        "failing",
        "0 0 0 * * *",
        chrono_tz::UTC,
        || async { Err(TalosError::internal("expected failure".to_string())) },
    )])
    .await
    .expect("job failure should not fail scheduler startup");

    automation
        .shutdown()
        .await
        .expect("shutdown should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_cron_fails_automation_startup() {
    let result = Automation::start(vec![AutomationJob::cron_on_start(
        "invalid",
        "not-a-cron",
        chrono_tz::UTC,
        || async { Ok(()) },
    )])
    .await;

    assert!(result
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
}

#[test]
fn cron_next_occurrence_preserves_iana_time_across_daylight_saving() {
    let schedule = parse_schedule("test", "0 30 3 * * *").unwrap();
    let now = Utc.with_ymd_and_hms(2026, 3, 7, 12, 0, 0).unwrap();

    let next = next_occurrence(&schedule, chrono_tz::America::New_York, now).unwrap();

    assert_eq!(next, Utc.with_ymd_and_hms(2026, 3, 8, 7, 30, 0).unwrap());
}
