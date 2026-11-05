use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

fn controlled_job(paused: bool, calls: Arc<AtomicUsize>) -> AutomationJob {
    AutomationJob::cron_on_start("native", "0 0 0 * * *", chrono_tz::UTC, move || {
        let calls = calls.clone();
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .with_control(AutomationJobControl {
        paused,
        run_once: true,
    })
}

#[tokio::test]
async fn persisted_pause_suppresses_startup_and_scheduled_admission() {
    let calls = Arc::new(AtomicUsize::new(0));
    let automation = Automation::start(vec![controlled_job(true, calls.clone())])
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(automation.jobs[0]
        .claim(Trigger::Scheduled)
        .unwrap()
        .is_none());
    let info = automation.jobs().await.unwrap().remove(0);
    assert!(info.paused && !info.running);
    assert_eq!(info.next_due_at, None);
    assert_eq!(info.last_finished_at, None);
    assert_eq!(info.last_error, None);
    automation.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paused_manual_run_is_single_claim_and_drains_without_resuming() {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let automation = Automation::start(vec![AutomationJob::cron(
        "native",
        "0 0 0 * * *",
        chrono_tz::UTC,
        {
            let started = started.clone();
            let release = release.clone();
            move || {
                let started = started.clone();
                let release = release.clone();
                async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(())
                }
            }
        },
    )
    .with_control(AutomationJobControl {
        paused: true,
        run_once: true,
    })])
    .await
    .unwrap();
    automation.run_once("native").await.unwrap();
    started.notified().await;
    assert!(automation.jobs().await.unwrap()[0].running);
    assert!(automation
        .run_once("native")
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    assert!(automation.jobs[0]
        .claim(Trigger::Scheduled)
        .unwrap()
        .is_none());
    let closing = {
        let automation = automation.clone();
        tokio::spawn(async move { automation.shutdown().await })
    };
    tokio::task::yield_now().await;
    assert!(!closing.is_finished());
    release.notify_one();
    closing.await.unwrap().unwrap();
    assert!(automation.jobs[0]
        .claim(Trigger::Scheduled)
        .unwrap()
        .is_none());
    let info = automation.jobs[0].snapshot();
    assert!(info.paused && !info.running && info.last_finished_at.is_some());
    assert_eq!(info.last_error, None);
    assert!(automation
        .run_once("native")
        .await
        .unwrap_err()
        .is_code(ErrorCode::ServiceUnavailable));
}

#[tokio::test]
async fn pause_does_not_cancel_running_and_resume_schedules_future_only() {
    let automation = Automation::start(vec![controlled_job(true, Arc::new(AtomicUsize::new(0)))])
        .await
        .unwrap();
    let running = automation.jobs[0].claim(Trigger::Manual).unwrap().unwrap();
    automation
        .set_paused(AutomationPauseUpdate {
            name: "native",
            paused: false,
        })
        .await
        .unwrap();
    let info = automation.jobs().await.unwrap().remove(0);
    assert!(!info.paused && info.running && info.next_due_at.unwrap() > Utc::now());
    automation
        .set_paused(AutomationPauseUpdate {
            name: "native",
            paused: true,
        })
        .await
        .unwrap();
    assert!(automation.jobs().await.unwrap()[0].running);
    running.run().await;
    assert!(automation.jobs().await.unwrap()[0].paused);
    automation.shutdown().await.unwrap();
}

#[tokio::test]
async fn callback_failure_is_observed_not_fabricated_or_retried() {
    let automation = Automation::start(vec![AutomationJob::cron_on_start(
        "native",
        "0 0 0 * * *",
        chrono_tz::UTC,
        || async {
            Err(TalosError::new(
                ErrorCode::Conflict,
                "Expected native failure.",
            ))
        },
    )
    .with_control(AutomationJobControl {
        paused: false,
        run_once: true,
    })])
    .await
    .unwrap();
    let info = automation.jobs().await.unwrap().remove(0);
    assert!(!info.running && info.last_finished_at.is_some());
    assert_eq!(info.last_error.unwrap().code, ErrorCode::Conflict);
    automation.shutdown().await.unwrap();
}

#[tokio::test]
async fn internal_pumps_and_unsupported_manual_actions_are_not_exposed() {
    let automation = Automation::start(vec![
        AutomationJob::cron("loop-scan", "0 0 0 * * *", chrono_tz::UTC, || async {
            Ok(())
        }),
        AutomationJob::cron("native", "0 0 0 * * *", chrono_tz::UTC, || async { Ok(()) })
            .with_control(AutomationJobControl {
                paused: false,
                run_once: false,
            }),
    ])
    .await
    .unwrap();
    assert_eq!(automation.jobs().await.unwrap().len(), 1);
    assert!(automation
        .run_once("loop-scan")
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    assert!(automation
        .set_paused(AutomationPauseUpdate {
            name: "loop-scan",
            paused: true
        })
        .await
        .unwrap_err()
        .is_code(ErrorCode::ResourceNotFound));
    assert!(automation
        .run_once("native")
        .await
        .unwrap_err()
        .is_code(ErrorCode::InvalidRequest));
    automation.shutdown().await.unwrap();
}

#[tokio::test]
async fn every_job_is_validated_before_any_startup_callback() {
    let calls = Arc::new(AtomicUsize::new(0));
    let result = Automation::start(vec![
        controlled_job(false, calls.clone()),
        AutomationJob::cron("bad", "bad", chrono_tz::UTC, || async { Ok(()) }),
    ])
    .await;
    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
